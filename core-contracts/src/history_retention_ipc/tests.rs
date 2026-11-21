use super::*;
use serde_json::{Value, json};

fn valid_policy() -> Value {
    json!({
        "enabled": true,
        "specifiedDays": 180,
        "suggestedDays": 90,
        "policyFingerprint": "0123456789abcdef0123456789abcdef"
    })
}

#[test]
fn request_requires_an_empty_object() {
    assert!(serde_json::from_value::<HistoryRetentionGetRequest>(json!({})).is_ok());
    for value in [json!({"extra": 1}), json!(null), json!([]), json!(0)] {
        assert!(serde_json::from_value::<HistoryRetentionGetRequest>(value).is_err());
    }
}

#[test]
fn policy_round_trips_with_omitted_optional_fields() -> Result<(), serde_json::Error> {
    let value = valid_policy();
    let policy: HistoryRetentionPolicy = serde_json::from_value(value.clone())?;
    assert_eq!(serde_json::to_value(policy)?, value);
    Ok(())
}

#[test]
fn policy_accepts_unknown_response_fields() {
    let mut response = valid_policy();
    response["futureField"] = json!(true);
    assert!(serde_json::from_value::<HistoryRetentionPolicy>(response).is_ok());
}

#[test]
fn policy_rejects_invalid_optional_fields_and_fingerprints() {
    let mut invalid = valid_policy();
    invalid["specifiedDays"] = Value::Null;
    assert!(serde_json::from_value::<HistoryRetentionPolicy>(invalid).is_err());

    let mut invalid = valid_policy();
    invalid["unavailableReason"] = Value::Null;
    assert!(serde_json::from_value::<HistoryRetentionPolicy>(invalid).is_err());

    let mut invalid = valid_policy();
    invalid["specifiedDays"] = json!(0);
    assert!(serde_json::from_value::<HistoryRetentionPolicy>(invalid).is_err());

    for fingerprint in ["", "0123456789ABCDEF0123456789ABCDEF", "0123456789abcdef"] {
        let mut invalid = valid_policy();
        invalid["policyFingerprint"] = json!(fingerprint);
        assert!(serde_json::from_value::<HistoryRetentionPolicy>(invalid).is_err());
    }

    for (enabled, days) in [(true, None), (false, Some(180))] {
        let mut invalid = json!({
            "enabled": enabled,
            "suggestedDays": 90,
            "policyFingerprint": "0123456789abcdef0123456789abcdef",
            "unavailableReason": "history.policy_invalid"
        });
        if let Some(days) = days {
            invalid["specifiedDays"] = json!(days);
        }
        assert!(serde_json::from_value::<HistoryRetentionPolicy>(invalid).is_err());
    }
}

#[test]
fn review_request_rejects_null_unknown_and_zero_days() {
    assert!(
        serde_json::from_value::<HistoryRetentionReviewRequest>(json!({"enabled":true})).is_ok()
    );
    for value in [
        json!({"enabled":true,"days":null}),
        json!({"enabled":true,"days":0}),
        json!({"enabled":false,"extra":1}),
    ] {
        assert!(serde_json::from_value::<HistoryRetentionReviewRequest>(value).is_err());
    }
}

#[test]
fn apply_requests_require_a_lowercase_fingerprint_and_reject_unknown_fields() {
    let id = "0195f856-ab83-7a59-9343-10d29775c9a9";
    let valid = json!({"reviewRef":id,"fingerprint":"0123456789abcdef0123456789abcdef"});
    assert!(serde_json::from_value::<HistoryRetentionApplyRequest>(valid.clone()).is_ok());
    for fingerprint in [
        "0123456789ABCDEF0123456789ABCDEF",
        "0123456789abcdef",
        "0123456789abcdef0123456789abcdef00",
    ] {
        let mut invalid = valid.clone();
        invalid["fingerprint"] = json!(fingerprint);
        assert!(serde_json::from_value::<HistoryRetentionApplyRequest>(invalid).is_err());
    }
    let mut invalid = valid;
    invalid["extra"] = json!(true);
    assert!(serde_json::from_value::<HistoryRetentionApplyRequest>(invalid).is_err());
}

#[test]
fn responses_omit_optional_values_and_accept_unknown_fields() -> Result<(), serde_json::Error> {
    let response = HistoryRetentionReview {
        review_ref: Id::from_v7_bytes([1; 16]),
        change_kind: HistoryRetentionChangeKind::None,
        confirmation_required: false,
        cleanup_after_apply: false,
        target_count: "0".into(),
        target_earliest_at_ms: None,
        target_latest_at_ms: None,
        held_count: "0".into(),
        undo_info_removal_count: "0".into(),
        fingerprint: "0123456789abcdef0123456789abcdef".into(),
    };
    let mut wire = serde_json::to_value(response)?;
    assert!(wire.get("targetEarliestAtMs").is_none());
    wire["futureField"] = serde_json::json!(true);
    assert!(serde_json::from_value::<HistoryRetentionReview>(wire).is_ok());

    let mut invalid = serde_json::to_value(HistoryRetentionReview {
        review_ref: Id::from_v7_bytes([1; 16]),
        change_kind: HistoryRetentionChangeKind::Enable,
        confirmation_required: true,
        cleanup_after_apply: true,
        target_count: "1".into(),
        target_earliest_at_ms: Some(10),
        target_latest_at_ms: Some(20),
        held_count: "0".into(),
        undo_info_removal_count: "0".into(),
        fingerprint: "0123456789abcdef0123456789abcdef".into(),
    })?;
    invalid["targetEarliestAtMs"] = Value::Null;
    assert!(serde_json::from_value::<HistoryRetentionReview>(invalid).is_err());

    let mut invalid = serde_json::to_value(HistoryRetentionReview {
        review_ref: Id::from_v7_bytes([1; 16]),
        change_kind: HistoryRetentionChangeKind::Enable,
        confirmation_required: true,
        cleanup_after_apply: true,
        target_count: "01".into(),
        target_earliest_at_ms: Some(10),
        target_latest_at_ms: Some(20),
        held_count: "0".into(),
        undo_info_removal_count: "0".into(),
        fingerprint: "0123456789abcdef0123456789abcdef".into(),
    })?;
    assert!(serde_json::from_value::<HistoryRetentionReview>(invalid.clone()).is_err());
    invalid["targetCount"] = serde_json::json!("00");
    assert!(serde_json::from_value::<HistoryRetentionReview>(invalid).is_err());
    Ok(())
}

#[test]
fn apply_and_cancel_responses_accept_unknown_fields_and_reject_explicit_nulls() {
    let response = json!({
        "outcome":"applied",
        "policy":{
            "enabled":true,
            "specifiedDays":30,
            "suggestedDays":90,
            "policyFingerprint":"0123456789abcdef0123456789abcdef"
        },
        "futureField":true
    });
    assert!(serde_json::from_value::<HistoryRetentionApplyResponse>(response.clone()).is_ok());
    let mut invalid = response;
    invalid["cleanupJobRef"] = Value::Null;
    assert!(serde_json::from_value::<HistoryRetentionApplyResponse>(invalid).is_err());

    assert!(
        serde_json::from_value::<HistoryRetentionCancelResponse>(json!({
            "outcome":"cancelled",
            "futureField":true
        }))
        .is_ok()
    );
}
