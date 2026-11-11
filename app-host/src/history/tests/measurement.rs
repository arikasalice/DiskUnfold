//! Opt-in native F02 fixture measurement; records observable event delivery, not UI latency.
use super::*;
use du_core_contracts::result_events::RESULT_VERSION_CHANGED;
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

/// Creates only temporary one-byte files and invokes the real desktop scan pipeline.
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
    let events = du_engine_api::desktop::register(
        &mut builder,
        Arc::new(du_platform_os::windows_observation::WindowsScopeRoots),
        Arc::new(du_platform_os::volume::WindowsVolumes),
        Arc::new(database.history(|| 400)),
        &du_platform_os::entropy::OsRandom,
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
    for run in 1..=2 {
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
                    terminal = Some((
                        sent.elapsed().as_millis(),
                        event.occurred_at.as_str().to_owned(),
                        event.body,
                    ));
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
            terminal.1,
            terminal.2,
            process_peak_working_set()
        );
        if terminal.2["state"] != "done" {
            return Err("scan did not finish successfully".into());
        }
        if terminal.2["completed"].as_u64()
            != Some((DIRECTORIES * (FILES_PER_DIRECTORY + 1)) as u64)
            || ["failed", "skipped", "unprocessed"]
                .iter()
                .any(|key| terminal.2[*key].as_u64() != Some(0))
        {
            return Err("scan did not account for the complete fixture".into());
        }
    }
    Ok(())
}
