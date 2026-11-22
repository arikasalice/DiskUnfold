use super::*;
use du_change_planner::{
    api::PlanBuilder,
    ports::{PlanSourceKind, RequestChannel},
    readiness::PlanReadiness,
};
use du_core_contracts::{Timestamp, data_source::DataSource};

fn id(seed: u8) -> Id {
    let mut bytes = [seed; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    Id::from_v7_bytes(bytes)
}

fn plan() -> Result<PlanVersion, String> {
    let mut value = PlanVersion {
        version_id: id(1),
        lineage_id: id(2),
        parent_version_id: None,
        source_version_id: id(3),
        source_kind: PlanSourceKind::UserSelection,
        data_source: DataSource::Real,
        request_channel: RequestChannel::Gui,
        ruleset: None,
        items: Vec::new(),
        expected_effect: Vec::new(),
        confirmations: Vec::new(),
        readiness: PlanReadiness::Ready,
        hash_format: "du.plan.hash.v1".into(),
        hash: [0; 32],
        created_at: Timestamp::from_unix_millis(1).ok_or("timestamp")?,
    };
    value.hash = PlanBuilder::hash_version(&value);
    Ok(value)
}

fn approval(
    value: PlanVersion,
    record_hash: [u8; 32],
    secret: [u8; 32],
) -> (ApprovalRecord, ApprovalExecutionToken, Id) {
    let session_id = id(4);
    let record = ApprovalRecord {
        approval_id: id(5),
        request_version_id: value.version_id.clone(),
        plan_hash: record_hash,
        hash_format: value.hash_format.clone(),
        app_session_id: session_id.clone(),
        secret,
        approved_plan: value.clone(),
        confirmations: None,
        ruleset_version: Some(7),
        state: ApprovalRecordState::Issued,
    };
    let token = ApprovalExecutionToken::issue(value.version_id.clone(), session_id.clone(), secret);
    (record, token, session_id)
}

#[test]
fn issued_approval_is_consumed_exactly_once() -> Result<(), String> {
    let value = plan()?;
    let (mut record, mut token, session) = approval(value.clone(), value.hash, [9; 32]);
    let receipt = check_approval(Some(&mut record), &mut token, &value, &session, None)
        .map_err(|error| format!("unexpected check failure: {error:?}"))?;
    assert_eq!(receipt.request_version_id, value.version_id);
    assert_eq!(record.state, ApprovalRecordState::Consumed);
    assert_eq!(record.secret, [0; 32]);
    assert_eq!(
        check_approval(Some(&mut record), &mut token, &value, &session, None),
        Err(ApprovalCheckFailure::AlreadyUsed)
    );
    Ok(())
}

#[test]
fn secret_mismatch_stops_before_consumption() -> Result<(), String> {
    let value = plan()?;
    let (mut record, mut token, session) = approval(value.clone(), value.hash, [9; 32]);
    token.secret[0] ^= 1;
    assert_eq!(
        check_approval(Some(&mut record), &mut token, &value, &session, None),
        Err(ApprovalCheckFailure::Mismatch)
    );
    assert_eq!(record.state, ApprovalRecordState::Issued);
    Ok(())
}

#[test]
fn recomputed_hash_mismatch_rejects_the_grant() -> Result<(), String> {
    let value = plan()?;
    let (mut record, mut token, session) = approval(value.clone(), [8; 32], [9; 32]);
    assert_eq!(
        check_approval(Some(&mut record), &mut token, &value, &session, None),
        Err(ApprovalCheckFailure::Mismatch)
    );
    assert_eq!(record.state, ApprovalRecordState::Issued);
    Ok(())
}

#[test]
fn check_result_values_match_the_audit_schema() {
    assert_eq!(ApprovalCheckFailure::NotIssued.as_str(), "not_issued");
    assert_eq!(ApprovalCheckFailure::AlreadyUsed.as_str(), "already_used");
    assert_eq!(ApprovalCheckFailure::Mismatch.as_str(), "mismatch");
    assert_eq!(
        ApprovalCheckFailure::ConfirmationMissing.as_str(),
        "confirmation_missing"
    );
}
