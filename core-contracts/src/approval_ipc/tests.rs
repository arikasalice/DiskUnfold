use super::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::fmt::Debug;

const ID: &str = "018f0000-0000-7000-8000-000000000001";
const TIME: &str = "2026-11-21T00:00:00.000Z";

fn round_trip<T>(value: Value) -> Result<(), Box<dyn std::error::Error>>
where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    let typed: T = serde_json::from_value(value.clone())?;
    assert_eq!(serde_json::to_value(typed)?, value);
    Ok(())
}

fn review_summary() -> Value {
    json!({
        "targetCount":u32::MAX,
        "targetSize":"18446744073709551615",
        "visibleSelectionCount":1,
        "hiddenSelectionCount":0,
        "hiddenDescendantCount":u32::MAX,
        "actions":["trash","move","delete"],
        "retentionConditions":[],
        "effectSummary":{},
        "protectionResults":[{
            "itemRef":{"resultVersion":ID,"itemNo":0},
            "disposition":"included",
            "reasonKey":"protect.held"
        }],
        "restorationLimitSummary":{}
    })
}

#[test]
fn approval_state_values_and_sample_reason_reuse() {
    assert_eq!(
        serde_json::to_value(RequestKind::MoveUndo).unwrap_or(Value::Null),
        json!("move_undo")
    );
    assert_eq!(
        serde_json::to_value(ApprovalState::InReview).unwrap_or(Value::Null),
        json!("in_review")
    );
    assert_eq!(
        serde_json::to_value(ApprovalEventState::Consumed).unwrap_or(Value::Null),
        json!("consumed")
    );
    assert_eq!(
        APPROVAL_SAMPLE_SOURCE_DENIED,
        crate::sample_ipc::APPROVAL_SAMPLE_SOURCE_DENIED
    );
    assert_eq!(APPROVAL_REVIEW, "approval.review");
    assert_eq!(APPROVAL_GRANT, "approval.grant");
    assert_eq!(APPROVAL_STATE_CHANGED, "approval.state_changed");
    assert_eq!(APPROVAL_REVIEW_WITHDRAWN, "approval.review_withdrawn");
    for (reason, key) in [
        (APPROVAL_NOT_ISSUED, "approval.not_issued"),
        (APPROVAL_ALREADY_USED, "approval.already_used"),
        (APPROVAL_INVALIDATED, "approval.invalidated"),
        (APPROVAL_MISMATCH, "approval.mismatch"),
        (APPROVAL_CHECK_FAILED, "approval.check_failed"),
        (APPROVAL_REQUEST_NOT_FOUND, "approval.request_not_found"),
        (APPROVAL_REQUEST_SUPERSEDED, "approval.request_superseded"),
        (APPROVAL_STALE, "approval.stale"),
        (APPROVAL_PROTECTION_DENIED, "approval.protection_denied"),
        (
            APPROVAL_PROTECTION_UNDETERMINED,
            "approval.protection_undetermined",
        ),
        (
            APPROVAL_CONFIRMATION_MISSING,
            "approval.confirmation_missing",
        ),
        (APPROVAL_REVIEW_UNAVAILABLE, "approval.review_unavailable"),
        (APPROVAL_PLAN_SUPERSEDED, "approval.plan_superseded"),
        (APPROVAL_RULESET_CHANGED, "approval.ruleset_changed"),
        (
            APPROVAL_RESTORE_INFO_CLEARED,
            "approval.restore_info_cleared",
        ),
        (
            APPROVAL_MOVE_UNDO_INFO_CLEARED,
            "approval.move_undo_info_cleared",
        ),
        (APPROVAL_USER_REVOKED, "approval.user_revoked"),
        (APPROVAL_IN_USE, "approval.in_use"),
    ] {
        assert_eq!(reason, key);
    }
    assert_eq!(
        serde_json::to_value(ConfirmationKind::ReleaseDefaultProtection).unwrap_or(Value::Null),
        json!("release_default_protection")
    );
    assert_eq!(
        serde_json::to_value(ConfirmationKind::Overwrite).unwrap_or(Value::Null),
        json!("overwrite")
    );
}

#[test]
fn approval_review_and_grant_shapes_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    round_trip::<ApprovalProtectionResult>(json!({
        "itemRef":{"resultVersion":ID,"itemNo":0},"disposition":"held"
    }))?;
    round_trip::<ApprovalProtectionSummary>(json!({
        "rulesetVersion":"0","classTableVersion":"0","unlockedClasses":[],
        "unlockedItemCount":u32::MAX,"unlockedBytes":"0","requiredBasicProtectionAck":false
    }))?;
    round_trip::<ApprovalOverwriteSummary>(json!({
        "overwriteItemCount":u32::MAX,"displacedBytes":"18446744073709551615",
        "requiredOverwriteAck":true
    }))?;
    let mut summary = review_summary();
    summary["protectionSummary"] = json!({
        "rulesetVersion":"1","classTableVersion":"2","unlockedClasses":["system"],
        "unlockedItemCount":1,"unlockedBytes":"5","requiredBasicProtectionAck":true
    });
    summary["overwriteSummary"] = json!({
        "overwriteItemCount":1,"displacedBytes":"10","requiredOverwriteAck":true
    });
    round_trip::<ApprovalReviewSummary>(summary)?;
    round_trip::<ApprovalReviewRequest>(json!({"requestVersionId":ID,"requestKind":"plan"}))?;
    let review_response = json!({
        "requestVersionId":ID,
        "summary":review_summary(),
        "planHash":"a".repeat(64),
        "approvalPossible":false,
        "unavailableReasonKeys":["approval.protection_denied"],
        "currentApprovalState":"in_review"
    });
    round_trip::<ApprovalReviewResponse>(review_response.clone())?;
    let mut future_review = review_response;
    future_review["futureField"] = json!(true);
    assert!(serde_json::from_value::<ApprovalReviewResponse>(future_review).is_ok());
    round_trip::<ApprovalConfirmationProofs>(json!({
        "permanentDeleteSelected":true,"confirmedTargetCount":u32::MAX,
        "confirmedTargetSize":"18446744073709551615","irreversibilityConfirmed":true,
        "basicProtectionUnlockConfirmed":true,"overwriteConfirmed":true,
        "confirmedOverwriteItemCount":u32::MAX,"confirmedDisplacedBytes":"0"
    }))?;
    round_trip::<ApprovalGrantRequest>(json!({
        "requestVersionId":ID,"presentedPlanHash":"f".repeat(64),
        "confirmations":{"basicProtectionUnlockConfirmed":false}
    }))?;
    round_trip::<ApprovalGrantResponse>(json!({"state":"granted"}))?;
    assert!(
        serde_json::from_value::<ApprovalGrantResponse>(json!({
            "state":"granted","futureField":true
        }))
        .is_ok()
    );
    let all_null: ApprovalConfirmationProofs = serde_json::from_value(json!({
        "permanentDeleteSelected":null,"confirmedTargetCount":null,
        "confirmedTargetSize":null,"irreversibilityConfirmed":null,
        "basicProtectionUnlockConfirmed":null,"overwriteConfirmed":null,
        "confirmedOverwriteItemCount":null,"confirmedDisplacedBytes":null
    }))?;
    assert_eq!(
        all_null,
        ApprovalConfirmationProofs {
            permanent_delete_selected: None,
            confirmed_target_count: None,
            confirmed_target_size: None,
            irreversibility_confirmed: None,
            basic_protection_unlock_confirmed: None,
            overwrite_confirmed: None,
            confirmed_overwrite_item_count: None,
            confirmed_displaced_bytes: None,
        }
    );
    let omitted: ApprovalGrantRequest = serde_json::from_value(json!({
        "requestVersionId":ID,"presentedPlanHash":"0".repeat(64),"confirmations":null
    }))?;
    assert!(omitted.confirmations.is_none());
    assert!(
        serde_json::from_value::<ApprovalGrantRequest>(json!({
            "requestVersionId":ID,"presentedPlanHash":"0".repeat(64),"extra":1
        }))
        .is_err()
    );
    Ok(())
}

#[test]
fn approval_event_accepts_future_fields_without_exposing_ids()
-> Result<(), Box<dyn std::error::Error>> {
    let event = json!({
        "requestVersionId":ID,"state":"revoked","reasonKey":"approval.user_revoked",
        "sequence":9_007_199_254_740_991u64,"occurredAt":TIME
    });
    round_trip::<ApprovalStateChanged>(event.clone())?;
    let mut future = event;
    future["futureField"] = json!(true);
    let decoded: ApprovalStateChanged = serde_json::from_value(future)?;
    assert_eq!(decoded.state, ApprovalEventState::Revoked);
    let encoded = serde_json::to_value(decoded)?;
    assert!(encoded.get("approvalId").is_none());
    assert!(encoded.get("futureField").is_none());
    Ok(())
}
