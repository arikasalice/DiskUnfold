use super::*;
use du_core_contracts::ErrorEnvelope;
use serde_json::json;

fn request(correlation_id: &Id) -> String {
    json!({
        "command": "du_call",
        "args": {"envelope": {
            "name": "scan.start",
            "contractVersion": ENVELOPE_VERSION,
            "correlationId": correlation_id,
            "body": {}
        }}
    })
    .to_string()
}

fn response(correlation_id: &Id, existing: bool, error: Option<ErrorEnvelope>) -> String {
    json!({
        "name": "scan.start",
        "contractVersion": ENVELOPE_VERSION,
        "correlationId": correlation_id,
        "body": {
            "jobRef": Id::from_v7_bytes([2; 16]),
            "existing": existing,
            "scopeVersion": Id::from_v7_bytes([3; 16]),
            "scopeFingerprint": "0123456789abcdef",
            "lineageId": null
        },
        "error": error
    })
    .to_string()
}

#[test]
fn receipt_keeps_the_actual_request_correlation_id() {
    let correlation_id = Id::from_v7_bytes([1; 16]);
    let parsed = new_scan_receipt(
        &request(&correlation_id),
        &response(&correlation_id, false, None),
    );

    assert_eq!(
        parsed.as_ref().map(|(_, correlation_id)| correlation_id),
        Some(&correlation_id)
    );
    assert_eq!(
        parsed.as_ref().map(|(receipt, _)| &receipt.job_ref),
        Some(&Id::from_v7_bytes([2; 16]))
    );
}

#[test]
fn observer_ignores_non_gui_rejected_reused_and_mismatched_replies() {
    let correlation_id = Id::from_v7_bytes([1; 16]);
    let other_correlation_id = Id::from_v7_bytes([4; 16]);
    let valid_request = request(&correlation_id);

    let rejected =
        ErrorEnvelope::from_reason("support.internal_error", correlation_id.clone(), Vec::new());
    assert!(
        new_scan_receipt(&valid_request, &response(&correlation_id, false, rejected),).is_none()
    );
    assert!(new_scan_receipt(&valid_request, &response(&correlation_id, true, None),).is_none());
    assert!(
        new_scan_receipt(
            &valid_request,
            &response(&other_correlation_id, false, None),
        )
        .is_none()
    );

    let non_gui_request = valid_request.replace("du_call", "other");
    assert!(new_scan_receipt(&non_gui_request, &response(&correlation_id, false, None),).is_none());
}

#[test]
fn handoff_at_pending_admission_limit_keeps_out_of_order_contexts_matched() {
    const PENDING_ADMISSIONS: u8 = 32;
    let (sender, receiver) = channel();
    let mut expected = HashMap::new();

    for index in (0..PENDING_ADMISSIONS).rev() {
        let job_ref = Id::from_v7_bytes([index; 16]);
        let correlation_id = Id::from_v7_bytes([index.wrapping_add(64); 16]);
        expected.insert(job_ref.clone(), correlation_id.clone());
        let sent = sender.send(AttemptContext {
            job_ref,
            correlation_id,
        });
        assert!(sent.is_ok());
    }

    let mut pending = HashMap::new();
    collect_attempt_contexts(&receiver, &mut pending);
    assert_eq!(pending.len(), usize::from(PENDING_ADMISSIONS));

    for (job_ref, correlation_id) in expected {
        assert_eq!(pending.remove(&job_ref), Some(correlation_id));
    }
    assert!(pending.is_empty());
}

#[test]
fn handoff_rejects_after_worker_receiver_is_closed() {
    let (sender, receiver) = channel();
    drop(receiver);

    assert!(
        sender
            .send(AttemptContext {
                job_ref: Id::from_v7_bytes([2; 16]),
                correlation_id: Id::from_v7_bytes([3; 16]),
            })
            .is_err()
    );
}

#[test]
fn database_gate_failures_use_published_reason_keys() {
    let job_ref = Id::from_v7_bytes([2; 16]);
    let correlation_id = Id::from_v7_bytes([3; 16]);

    let busy = PersistFailure::from_recent_error(RecentError::Busy)
        .with_context(job_ref.clone(), correlation_id.clone());
    let full = PersistFailure::from_recent_error(RecentError::DiskFull)
        .with_context(job_ref.clone(), correlation_id.clone());
    let other = PersistFailure::from_recent_error(RecentError::Storage)
        .with_context(job_ref.clone(), correlation_id);

    assert_eq!(busy.reason_key(), "db.busy");
    assert_eq!(full.reason_key(), "db.disk_full");
    assert_eq!(other.reason_key(), "scope.recent_store_failed");
}
