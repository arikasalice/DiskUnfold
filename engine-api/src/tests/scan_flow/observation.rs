//! A finished GUI scan stores one observation; the log's failure leaves the scan alone.
use super::*;
use du_analysis_index::fingerprint_from_hex;
use du_core_contracts::{
    job::JobState,
    scope_observation_log::{
        ScopeObservation, ScopeObservationAppend, ScopeObservationLog, ScopeObservationWriteError,
    },
};

#[derive(Default)]
struct Log {
    rows: Mutex<Vec<ScopeObservation>>,
    fail: Option<ScopeObservationWriteError>,
}

impl ScopeObservationLog for Log {
    fn append(
        &self,
        observation: &ScopeObservation,
    ) -> Result<ScopeObservationAppend, ScopeObservationWriteError> {
        if let Some(error) = self.fail {
            return Err(error);
        }
        self.rows
            .lock()
            .map_err(|_| ScopeObservationWriteError::Storage)?
            .push(observation.clone());
        Ok(ScopeObservationAppend::Added)
    }
}

fn run_one_scan(
    harness: &ScanHarness,
) -> Result<(ScanStart, Box<JobSummary>, bool), Box<dyn std::error::Error>> {
    let gui = |name, body| call(&harness.engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    let raw: Vec<u8> = "C:\\Fixture"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let draft = json!({"includes": [{"encoding":"windows_utf16le", "raw":PathBytes::from_bytes(&raw), "display":"", "lossy":false}], "exclusions":[], "boundary":"current_volume", "includedMounts":[], "source":"gui"});
    let review: ScopeReview =
        serde_json::from_value(gui("scope.review", json!({"draft":draft, "requestSeq":1}))?.body)?;
    let receipt: ScanStart = serde_json::from_value(
        gui(
            "scan.start",
            json!({"draft":draft, "reviewFingerprint":review.review_fingerprint}),
        )?
        .body,
    )?;
    harness.release.send(())?;
    let (summary, history_recorded) = scan_finished(&harness.terminal)?;
    Ok((receipt, summary, history_recorded))
}

#[test]
fn a_finished_scan_leaves_one_row_read_from_its_sealed_result()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = scan_harness()?;
    let log = Arc::new(Log::default());
    harness.scan_events.bind_observation_log(log.clone())?;
    let (receipt, summary, history_recorded) = run_one_scan(&harness)?;
    assert_eq!(summary.state, JobState::Done);
    assert!(history_recorded);
    let rows = log.rows.lock().map_err(|_| "log poisoned")?.clone();
    let [row] = rows.as_slice() else {
        return Err(format!("expected one observation, found {}", rows.len()).into());
    };
    let version = harness
        .versions
        .sealed_scan_result(summary.result_version_id.as_ref().ok_or("missing result")?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(row.job_id, receipt.job_ref);
    assert_eq!(
        Some(row.scope_fingerprint),
        fingerprint_from_hex(&receipt.scope_fingerprint)
    );
    assert_eq!(
        Some(row.observed_at_ms),
        version.observed_at().map(|time| time.unix_millis())
    );
    assert_eq!(row.completeness, version.completeness());
    assert_eq!(row.size_rule_id, version.size_rule().id);
    Ok(())
}

#[test]
fn a_failing_log_leaves_the_scan_result_and_job_state_unchanged()
-> Result<(), Box<dyn std::error::Error>> {
    for error in [
        ScopeObservationWriteError::Busy,
        ScopeObservationWriteError::Invalid,
    ] {
        let harness = scan_harness()?;
        let log = Arc::new(Log {
            rows: Mutex::default(),
            fail: Some(error),
        });
        harness.scan_events.bind_observation_log(log.clone())?;
        let (receipt, summary, history_recorded) = run_one_scan(&harness)?;
        assert_eq!((summary.state, history_recorded), (JobState::Done, true));
        assert!(log.rows.lock().map_err(|_| "log poisoned")?.is_empty());
        let version = summary.result_version_id.ok_or("missing result")?;
        // The sealed result is still current and still confirmed by the terminal event.
        assert!(harness.completed_scans.admit(&version).is_ok());
        let status = call(
            &harness.engine,
            "job.status",
            Channel::Gui,
            ENVELOPE_VERSION,
            json!({"jobRef": receipt.job_ref}),
        )?;
        assert_eq!(status.body["state"], "done");
        assert_eq!(status.body["terminal"]["historyRecorded"], true);
        assert!(status.body["terminal"].get("warning").is_none());
    }
    Ok(())
}

#[test]
fn a_desktop_without_a_log_records_nothing_and_binds_only_once()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = scan_harness()?;
    let (_, summary, _) = run_one_scan(&harness)?;
    assert_eq!(summary.state, JobState::Done);
    let log = Arc::new(Log::default());
    harness.scan_events.bind_observation_log(log.clone())?;
    assert!(
        harness
            .scan_events
            .bind_observation_log(log.clone())
            .is_err()
    );
    assert!(log.rows.lock().map_err(|_| "log poisoned")?.is_empty());
    Ok(())
}
