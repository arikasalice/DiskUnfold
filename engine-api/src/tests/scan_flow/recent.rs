use super::*;
use du_core_contracts::scan_observation::{
    OptionalRecentScopeObserver, RecentScopeEvidence, RecentScopeObservation, RecentScopeObserver,
};

pub(super) fn open_with_observer(
    roots: &Roots,
    path: &[u8],
) -> Result<(OpenedScopeRoot, OptionalRecentScopeObserver), PortError> {
    roots.recent_opens.fetch_add(1, Ordering::Relaxed);
    let root = roots.open_root(path)?;
    let observer = RecentObserver {
        count: roots.recent_observations.clone(),
        live_handles: roots.recent_live_handles.clone(),
        volume_id: root.volume_id.clone(),
        entry_path: root.canonical_path.clone(),
        file_id: root.file_id.clone(),
    };
    observer.live_handles.fetch_add(1, Ordering::AcqRel);
    Ok((root, Some(Ok(Box::new(observer)))))
}

struct RecentObserver {
    count: Arc<AtomicUsize>,
    live_handles: Arc<AtomicUsize>,
    volume_id: Vec<u8>,
    entry_path: OsPath,
    file_id: Vec<u8>,
}

impl Drop for RecentObserver {
    fn drop(&mut self) {
        self.live_handles.fetch_sub(1, Ordering::AcqRel);
    }
}

impl RecentScopeObserver for RecentObserver {
    fn observe(&self, cancelled: &std::sync::atomic::AtomicBool) -> RecentScopeObservation {
        if cancelled.load(Ordering::Acquire) {
            return RecentScopeObservation::Cancelled;
        }
        self.count.fetch_add(1, Ordering::Relaxed);
        RecentScopeObservation::Observed(RecentScopeEvidence {
            volume_id: self.volume_id.clone(),
            entry_path: self.entry_path.clone(),
            relative_path: OsPath {
                encoding: PathEncoding::WindowsUtf16le,
                raw: PathBytes::from_bytes(&[]),
                display: String::new(),
                lossy: false,
            },
            file_id: self.file_id.clone(),
        })
    }
}

fn draft() -> serde_json::Value {
    let path: Vec<_> = "C:\\Fixture"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    json!({
        "includes": [{
            "encoding": "windows_utf16le",
            "raw": PathBytes::from_bytes(&path),
            "display": "untrusted",
            "lossy": true
        }],
        "exclusions": [],
        "boundary": "current_volume",
        "includedMounts": [],
        "source": "gui"
    })
}

#[test]
fn normal_scan_entry_does_not_create_recent_observer() -> Result<(), Box<dyn std::error::Error>> {
    let harness = scan_harness()?;
    let call = |name, body| call(&harness.engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    let reviewed = call("scope.review", json!({"draft": draft(), "requestSeq": 1}))?;
    let review: ScopeReview = serde_json::from_value(reviewed.body)?;
    let receipt: ScanStart = serde_json::from_value(
        call(
            "scan.start",
            json!({"draft": draft(), "reviewFingerprint": review.review_fingerprint}),
        )?
        .body,
    )?;
    assert_eq!(harness.roots.recent_observations.load(Ordering::Acquire), 0);
    assert_eq!(harness.roots.recent_opens.load(Ordering::Acquire), 0);
    harness.release.send(())?;
    let (summary, _) = scan_finished(&harness.terminal)?;
    assert_eq!(summary.job_id, receipt.job_ref);
    assert_eq!(harness.roots.recent_observations.load(Ordering::Acquire), 0);
    assert_eq!(harness.roots.recent_opens.load(Ordering::Acquire), 0);
    Ok(())
}

#[test]
fn recent_observation_waits_for_response_ack_and_authoritative_job_finish()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = scan_harness_with_recent_admissions()?;
    let admissions = harness
        .recent_scope_admissions
        .as_ref()
        .ok_or("recent admission queue missing")?;
    let call = |name, body| call(&harness.engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    let reviewed = call("scope.review", json!({"draft": draft(), "requestSeq": 1}))?;
    let review: ScopeReview = serde_json::from_value(reviewed.body)?;
    let start = || {
        call(
            "scan.start",
            json!({"draft": draft(), "reviewFingerprint": review.review_fingerprint}),
        )
    };
    let receipt: ScanStart = serde_json::from_value(start()?.body)?;
    let duplicate: ScanStart = serde_json::from_value(start()?.body)?;
    assert!(duplicate.existing);
    assert_eq!(duplicate.job_ref, receipt.job_ref);
    assert_eq!(harness.roots.recent_observations.load(Ordering::Acquire), 0);
    assert!(matches!(
        admissions.try_next(),
        Err(mpsc::TryRecvError::Empty)
    ));

    assert!(admissions.response_sent(&receipt.job_ref));
    let admission = admissions.try_next()?;
    assert_eq!(
        admission.progress(),
        crate::desktop::RecentScopeAdmissionProgress::Running
    );
    assert!(
        admission
            .observe(&std::sync::atomic::AtomicBool::new(false))
            .is_none()
    );
    assert_eq!(harness.roots.recent_observations.load(Ordering::Acquire), 0);
    harness.release.send(())?;
    let (summary, _) = scan_finished(&harness.terminal)?;
    assert_eq!(summary.job_id, receipt.job_ref);
    assert!(matches!(
        admissions.try_next(),
        Err(mpsc::TryRecvError::Empty)
    ));

    let evidence = admission
        .observe(&std::sync::atomic::AtomicBool::new(false))
        .ok_or("completed GUI admission did not produce recent-scope evidence")?;
    assert_eq!(evidence.len(), 1);
    assert_eq!(
        admission.progress(),
        crate::desktop::RecentScopeAdmissionProgress::Ready
    );
    assert_eq!(evidence[0].entry_path.display, "C:\\Fixture");
    assert_eq!(evidence[0].file_id, 1usize.to_le_bytes());
    assert_eq!(harness.roots.recent_observations.load(Ordering::Acquire), 1);
    Ok(())
}

#[test]
fn rejected_or_cancelled_scan_start_has_no_worker_handoff_or_observation()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = scan_harness_with_recent_admissions()?;
    let admissions = harness
        .recent_scope_admissions
        .as_ref()
        .ok_or("recent admission queue missing")?;
    let call = |name, body| call(&harness.engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    let reviewed = call("scope.review", json!({"draft": draft(), "requestSeq": 1}))?;
    let review: ScopeReview = serde_json::from_value(reviewed.body)?;
    let rejected = call(
        "scan.start",
        json!({"draft": draft(), "reviewFingerprint": "00".repeat(32)}),
    )?;
    assert_eq!(
        rejected
            .error
            .ok_or("expected rejected admission")?
            .reason_key,
        "scope.changed"
    );
    assert!(matches!(
        admissions.try_next(),
        Err(mpsc::TryRecvError::Empty)
    ));

    let receipt: ScanStart = serde_json::from_value(
        call(
            "scan.start",
            json!({"draft": draft(), "reviewFingerprint": review.review_fingerprint}),
        )?
        .body,
    )?;
    assert!(admissions.cancel_before_response(&receipt.job_ref));
    assert!(!admissions.response_sent(&receipt.job_ref));
    assert!(matches!(
        admissions.try_next(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert_eq!(harness.roots.recent_observations.load(Ordering::Acquire), 0);
    harness.release.send(())?;
    let _ = scan_finished(&harness.terminal)?;
    Ok(())
}

#[test]
fn runtime_cancelled_job_cannot_be_observed_for_recent_scope()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = scan_harness_with_recent_admissions()?;
    let admissions = harness
        .recent_scope_admissions
        .as_ref()
        .ok_or("recent admission queue missing")?;
    let call = |name, body| call(&harness.engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    let reviewed = call("scope.review", json!({"draft": draft(), "requestSeq": 1}))?;
    let review: ScopeReview = serde_json::from_value(reviewed.body)?;
    let receipt: ScanStart = serde_json::from_value(
        call(
            "scan.start",
            json!({"draft": draft(), "reviewFingerprint": review.review_fingerprint}),
        )?
        .body,
    )?;
    assert!(admissions.response_sent(&receipt.job_ref));
    let admission = admissions.try_next()?;
    let cancelled = call("job.cancel", json!({"jobRef": receipt.job_ref}))?;
    assert!(cancelled.error.is_none());
    harness.release.send(())?;
    let (summary, _) = scan_finished(&harness.terminal)?;
    assert_eq!(summary.state, du_core_contracts::job::JobState::Cancelled);
    assert!(
        admission
            .observe(&std::sync::atomic::AtomicBool::new(false))
            .is_none()
    );
    assert_eq!(
        admission.progress(),
        crate::desktop::RecentScopeAdmissionProgress::TerminalWithoutEvidence
    );
    assert_eq!(harness.roots.recent_observations.load(Ordering::Acquire), 0);
    Ok(())
}

#[test]
fn receiver_drop_releases_pending_handle_without_rejecting_scan_start()
-> Result<(), Box<dyn std::error::Error>> {
    let mut harness = scan_harness_with_recent_admissions()?;
    let call = |name, body| call(&harness.engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    let reviewed = call("scope.review", json!({"draft": draft(), "requestSeq": 1}))?;
    let review: ScopeReview = serde_json::from_value(reviewed.body)?;
    let receipt: ScanStart = serde_json::from_value(
        call(
            "scan.start",
            json!({"draft": draft(), "reviewFingerprint": review.review_fingerprint}),
        )?
        .body,
    )?;
    assert_eq!(harness.roots.recent_live_handles.load(Ordering::Acquire), 1);
    drop(harness.recent_scope_admissions.take());
    assert_eq!(harness.roots.recent_live_handles.load(Ordering::Acquire), 0);
    harness.release.send(())?;
    let (summary, _) = scan_finished(&harness.terminal)?;
    assert_eq!(summary.job_id, receipt.job_ref);
    Ok(())
}

#[test]
fn delayed_worker_keeps_capacity_without_rejecting_scan_start()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = scan_harness_with_recent_admissions()?;
    let admissions = harness
        .recent_scope_admissions
        .as_ref()
        .ok_or("recent admission queue missing")?;
    let call = |name, body| call(&harness.engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    let reviewed = call("scope.review", json!({"draft": draft(), "requestSeq": 1}))?;
    let review: ScopeReview = serde_json::from_value(reviewed.body)?;
    let first: ScanStart = serde_json::from_value(
        call(
            "scan.start",
            json!({"draft": draft(), "reviewFingerprint": review.review_fingerprint}),
        )?
        .body,
    )?;
    harness.release.send(())?;
    let _ = scan_finished(&harness.terminal)?;
    assert!(admissions.response_sent(&first.job_ref));
    let first_admission = admissions.try_next()?;

    let mut other = draft();
    let raw: Vec<_> = "C:\\Other"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    other["includes"][0]["raw"] = serde_json::to_value(PathBytes::from_bytes(&raw))?;
    let reviewed = call("scope.review", json!({"draft": other, "requestSeq": 2}))?;
    let review: ScopeReview = serde_json::from_value(reviewed.body)?;
    let second: ScanStart = serde_json::from_value(
        call(
            "scan.start",
            json!({"draft": other, "reviewFingerprint": review.review_fingerprint}),
        )?
        .body,
    )?;
    assert!(!second.existing);
    assert!(!admissions.response_sent(&second.job_ref));
    harness.release.send(())?;
    let (summary, _) = scan_finished(&harness.terminal)?;
    assert_eq!(summary.job_id, second.job_ref);
    drop(first_admission);
    Ok(())
}
