//! Linux scope review, scan, results and failures through the real adapters.
use super::*;
use du_core_contracts::{
    EventEnvelope, Id, PathBytes, ResultEnvelope, SizeAmount, SizeBasis,
    analysis_ipc::{AnalysisJobStatus, ScanStart, ScopeReview, SpaceTotal, TargetAccess},
    job::JobState,
    scan_failures::ScanFailures,
};
use du_engine_api::desktop::SharedTableLimit;
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    fs,
    os::unix::{
        ffi::OsStrExt,
        fs::{PermissionsExt, symlink},
    },
    path::Path,
    time::{Duration, Instant},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

mod exclusions;

fn ipc(
    engine: &Engine,
    name: &str,
    body: Value,
) -> Result<ResultEnvelope, Box<dyn std::error::Error>> {
    let id = Id::from_v7_bytes([42; 16]);
    let message = json!({
        "command": "du_call",
        "args": {"envelope": {
            "name": name, "contractVersion": 1, "correlationId": id, "body": body
        }}
    });
    let response = crate::ipc::dispatch(engine, &message.to_string()).ok_or("no response")?;
    Ok(serde_json::from_str(&response)?)
}

fn read<T: serde::de::DeserializeOwned>(
    engine: &Engine,
    name: &str,
    body: Value,
) -> Result<T, Box<dyn std::error::Error>> {
    let response = ipc(engine, name, body)?;
    if let Some(error) = response.error {
        return Err(format!("{name}: {error:?}").into());
    }
    Ok(serde_json::from_value(response.body)?)
}

fn finished(events: &DesktopEvents, job: &Id) -> Result<EventEnvelope, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        match events.next_json() {
            Some(json) => {
                let event: EventEnvelope = serde_json::from_str(&json)?;
                if event.topic == "job.finished" && event.job_ref.as_ref() == Some(job) {
                    return Ok(event);
                }
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    Err("timed out waiting for the scan to finish".into())
}

/// Returns the logical bytes and whether the total is exact rather than a lower bound.
fn logical(total: &SpaceTotal) -> Result<(u64, bool), Box<dyn std::error::Error>> {
    let size = total
        .sizes
        .iter()
        .find(|value| value.size.basis == SizeBasis::Logical)
        .ok_or("missing logical size")?;
    match &size.size.amount {
        SizeAmount::Exact { bytes } => Ok((bytes.as_str().parse()?, true)),
        SizeAmount::LowerBound { bytes } => Ok((bytes.as_str().parse()?, false)),
        other => Err(format!("unexpected logical size: {other:?}").into()),
    }
}

/// Builds the scope: 135 readable bytes, two links and an optional denied folder.
fn fixture(base: &Path, denied: bool) -> TestResult {
    fs::create_dir_all(base.join("sub").join("deep"))?;
    fs::create_dir(base.join("empty"))?;
    fs::write(base.join("a.bin"), vec![1u8; 100])?;
    fs::write(base.join("sub").join("b.bin"), vec![2u8; 23])?;
    fs::write(base.join("sub").join("deep").join("c.bin"), vec![3u8; 7])?;
    fs::write(base.join("zero"), b"")?;
    fs::write(base.join(OsStr::from_bytes(b"raw-\xff")), vec![4u8; 5])?;
    symlink("..", base.join("up"))?;
    symlink("/", base.join("outside"))?;
    if denied {
        fs::create_dir(base.join("locked"))?;
        fs::write(base.join("locked").join("hidden"), vec![9u8; 50])?;
        fs::set_permissions(base.join("locked"), fs::Permissions::from_mode(0o000))?;
    }
    Ok(())
}

#[test]
fn linux_scan_reviews_collects_and_reports_failures() -> TestResult {
    let denied = std::env::var_os("DU_TEST_UNPRIVILEGED").is_some();
    let temp = tempfile::tempdir()?;
    let profile = tempfile::tempdir()?;
    let base = temp.path().join("scope");
    fixture(&base, denied)?;
    let outcome = scan(&base, profile.path(), denied);
    if denied {
        fs::set_permissions(base.join("locked"), fs::Permissions::from_mode(0o700))?;
    }
    outcome
}

fn scan(base: &Path, profile: &Path, denied: bool) -> TestResult {
    let ports = scan_ports().ok_or("Linux collection adapters are not injected")?;
    let database = crate::startup_database::open(profile)?;
    let mut builder = EngineBuilder::default();
    let limit: SharedTableLimitSource = Arc::new(SharedTableLimit::default);
    let services = register_analysis(&mut builder, ports, &database, limit, None)?;
    let engine = builder.build()?;
    verify_registered(&engine, true)?;
    let include = json!({
        "encoding": "unix",
        "raw": PathBytes::from_bytes(base.as_os_str().as_bytes()),
        "display": base.to_string_lossy(),
        "lossy": false
    });
    let draft = json!({"includes":[include],"exclusions":[],"boundary":"current_volume","includedMounts":[],"source":"gui"});
    let review: ScopeReview = read(
        &engine,
        "scope.review",
        json!({"draft":draft,"requestSeq":1}),
    )?;
    assert_eq!(review.targets.len(), 1);
    assert_eq!(review.targets[0].access, TargetAccess::Accessible);
    let start: ScanStart = read(
        &engine,
        "scan.start",
        json!({"draft":draft,"reviewFingerprint":review.review_fingerprint}),
    )?;
    let finish = finished(&services.events, &start.job_ref)?;
    assert_eq!(finish.body["state"], "done");
    let status: AnalysisJobStatus = read(&engine, "job.get", json!({"jobRef":start.job_ref}))?;
    assert_eq!(status.state, JobState::Done);
    let version = status.result_version.ok_or("missing result version")?;
    let total: SpaceTotal = read(
        &engine,
        "space.total",
        json!({"resultVersion":version,"requestSeq":1,"target":{"kind":"scope_root"}}),
    )?;
    // Untraversed links leave the readable total as a lower bound.
    let (bytes, exact) = logical(&total)?;
    assert_eq!((bytes, exact), (135, false));
    let failures: ScanFailures = read(&engine, "scan.failures", json!({"resultVersion":version}))?;
    let rows: Vec<_> = failures
        .rows
        .iter()
        .map(|row| {
            (
                row.display_path.clone().unwrap_or_default(),
                row.reason_key.clone(),
            )
        })
        .collect();
    let link_rows = rows
        .iter()
        .filter(|(_, reason)| reason == "scan.link_untraversed")
        .count();
    assert_eq!(link_rows, 2, "{rows:?}");
    let denied_rows = rows
        .iter()
        .filter(|(path, reason)| path.ends_with("/locked") && reason == "scan.path_denied")
        .count();
    assert_eq!(denied_rows, usize::from(denied), "{rows:?}");
    assert_eq!(failures.total_count, 2 + u64::from(denied));
    eprintln!(
        "F02_LINUX_ENGINE state=done logical_lower_bound={bytes} completed={} failed={} failure_rows={} denied_case={denied}",
        finish.body["completed"], finish.body["failed"], failures.total_count
    );
    Ok(())
}
