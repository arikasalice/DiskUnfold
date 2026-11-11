//! Opt-in native F02 fixture measurement; records observable event delivery, not UI latency.
use super::*;
use du_core_contracts::{
    SizeAmount, SizeBasis, analysis_ipc::SpaceTotal, result::ResultCompleteness,
    result_events::RESULT_VERSION_CHANGED,
};
use std::fs;

/// Reads the process-lifetime peak working set, which is not scan-job memory.
fn process_peak_working_set() -> Option<u64> {
    let query = format!("(Get-Process -Id {}).PeakWorkingSet64", std::process::id());
    let result = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &query])
        .output()
        .ok()?;
    result.status.success().then_some(())?;
    std::str::from_utf8(&result.stdout)
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Checks the exact logical total through the desktop query pinned to the finished scan.
fn verify_pinned_logical_total(
    engine: &Engine,
    version: &Id,
    request_seq: u32,
    expected_bytes: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = ipc(
        engine,
        "space.total",
        json!({"resultVersion":version,"requestSeq":request_seq,"target":{"kind":"scope_root"}}),
    )?;
    if let Some(error) = response.error {
        return Err(format!("space.total failed: {error:?}").into());
    }
    let total: SpaceTotal = serde_json::from_value(response.body)?;
    assert_eq!(&total.result_version, version);
    assert_eq!(total.request_seq, request_seq);
    assert_eq!(total.completeness, ResultCompleteness::Complete);
    let logical = total
        .sizes
        .iter()
        .find(|entry| entry.size.basis == SizeBasis::Logical)
        .ok_or("missing logical size")?;
    assert_eq!(logical.completeness, ResultCompleteness::Complete);
    let SizeAmount::Exact { bytes } = &logical.size.amount else {
        return Err(format!("logical size is not exact: {:?}", logical.size.amount).into());
    };
    assert_eq!(bytes.as_str().parse::<u64>()?, expected_bytes);
    Ok(())
}

/// Creates temporary one-byte files and queries each final result via desktop IPC.
/// Run with `cargo test --locked -p du-app-host --features gui --lib
/// history::tests::measurement::native_scan_100k -- --ignored --nocapture`.
/// Fixture creation warms an uncontrolled OS cache; no performance target is asserted.
#[test]
#[ignore = "opt-in 100,000-file native Windows measurement"]
fn native_scan_100k() -> Result<(), Box<dyn std::error::Error>> {
    const DIRECTORIES: usize = 100;
    const FILES_PER_DIRECTORY: usize = 1_000;
    let fixture = tempfile::tempdir()?;
    let profile = tempfile::tempdir()?;
    for directory in 0..DIRECTORIES {
        let path = fixture.path().join(format!("dir-{directory:03}"));
        fs::create_dir(&path)?;
        for file in 0..FILES_PER_DIRECTORY {
            fs::write(path.join(format!("entry-{file:04}.bin")), [1])?;
        }
    }
    let database = crate::startup_database::open(profile.path())?;
    let mut builder = EngineBuilder::default();
    let diagnostics = Arc::new(std::sync::Mutex::new(Vec::new()));
    let capture = diagnostics.clone();
    let events = du_engine_api::desktop::register_with_scan_diagnostics(
        &mut builder,
        Arc::new(du_platform_os::windows_observation::WindowsScopeRoots),
        Arc::new(du_platform_os::volume::WindowsVolumes),
        Arc::new(database.history(|| 400)),
        &du_platform_os::entropy::OsRandom,
        Arc::new(move || {
            let scan = Arc::new(du_engine_api::desktop::ScanDiagnostics::default());
            capture
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(scan.clone());
            scan
        }),
    )?;
    let engine = builder.build()?;
    let raw: Vec<_> = fixture
        .path()
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let draft = json!({"includes":[{"encoding":"windows_utf16le", "raw":PathBytes::from_bytes(&raw), "display":fixture.path(), "lossy":false}], "exclusions":[], "boundary":"current_volume", "includedMounts":[], "source":"gui"});
    eprintln!(
        "F02_MEASURE fixture_files={} directories={} bytes_per_file=1 cache=uncontrolled_after_fixture_creation workers=default pid={} pre_scan_process_lifetime_peak_working_set_bytes={:?}",
        DIRECTORIES * FILES_PER_DIRECTORY,
        DIRECTORIES,
        std::process::id(),
        process_peak_working_set()
    );
    let mut previous_version = None;
    for run in 1..=2_u32 {
        if run == 2 {
            // Keep the entry count constant while making reuse of the first aggregate observable.
            fs::write(fixture.path().join("dir-000/entry-0000.bin"), [1, 2])?;
        }
        let review = ipc(
            &engine,
            "scope.review",
            json!({"draft":draft,"requestSeq":run}),
        )?;
        if let Some(error) = review.error {
            return Err(format!("review failed: {error:?}").into());
        }
        let review: ScopeReview = serde_json::from_value(review.body)?;
        let sent = Instant::now();
        let response = ipc(
            &engine,
            "scan.start",
            json!({"draft":draft,"reviewFingerprint":review.review_fingerprint}),
        )?;
        let receipt_ms = sent.elapsed().as_millis();
        if let Some(error) = response.error {
            return Err(format!("scan start failed: {error:?}").into());
        }
        let receipt: ScanStart = serde_json::from_value(response.body)?;
        if receipt.existing {
            return Err("measurement unexpectedly reused a scan".into());
        }
        let deadline = Instant::now() + Duration::from_secs(1_200);
        let mut first = None;
        let mut terminal = None;
        while Instant::now() < deadline {
            if let Some(json) = events.next_json() {
                let event: EventEnvelope = serde_json::from_str(&json)?;
                if event.topic == RESULT_VERSION_CHANGED
                    && event.body["lineageId"].as_str()
                        == receipt.lineage_id.as_ref().map(Id::as_str)
                    && first.is_none()
                {
                    first = Some((
                        sent.elapsed().as_millis(),
                        event.occurred_at.as_str().to_owned(),
                        event.body.clone(),
                    ));
                }
                if event.topic == "job.finished" && event.job_ref.as_ref() == Some(&receipt.job_ref)
                {
                    terminal = Some((sent.elapsed().as_millis(), event));
                    break;
                }
            } else {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        let first = first.ok_or("missing first result notification")?;
        let terminal = terminal.ok_or("missing terminal notification")?;
        eprintln!(
            "F02_MEASURE run={run} receipt_ms={receipt_ms} first_notification_dequeued_ms={} first_notification_occurred_at={} first_body={} terminal_notification_dequeued_ms={} terminal_notification_occurred_at={} terminal_body={} process_lifetime_peak_working_set_bytes={:?}",
            first.0,
            first.1,
            first.2,
            terminal.0,
            terminal.1.occurred_at.as_str(),
            terminal.1.body,
            process_peak_working_set()
        );
        let peak = diagnostics
            .lock()
            .map_err(|_| "diagnostics lock poisoned")?
            .get((run - 1) as usize)
            .and_then(|scan| scan.shared_ready_peak())
            .ok_or("missing ready queue peak")?;
        eprintln!(
            "F02_MEASURE run={run} shared_ready_queue_peak_frames={peak} excludes=worker_local_overflow,inflight,observation_channel"
        );
        let body = &terminal.1.body;
        if body["state"] != "done" {
            return Err("scan did not finish successfully".into());
        }
        if body["completed"].as_u64() != Some((DIRECTORIES * (FILES_PER_DIRECTORY + 1)) as u64)
            || ["failed", "skipped", "unprocessed"]
                .iter()
                .any(|key| body[*key].as_u64() != Some(0))
        {
            return Err("scan did not account for the complete fixture".into());
        }
        let version = terminal
            .1
            .result_version
            .ok_or("missing final result version")?;
        assert_ne!(previous_version.as_ref(), Some(&version));
        let expected_bytes = (DIRECTORIES * FILES_PER_DIRECTORY) as u64 + u64::from(run - 1);
        let query_start = Instant::now();
        verify_pinned_logical_total(&engine, &version, run, expected_bytes)?;
        eprintln!(
            "F03_MEASURE run={run} pinned_result_version={} logical_exact_bytes={expected_bytes} query_and_validation_us={}",
            version.as_str(),
            query_start.elapsed().as_micros()
        );
        previous_version = Some(version);
    }
    Ok(())
}
