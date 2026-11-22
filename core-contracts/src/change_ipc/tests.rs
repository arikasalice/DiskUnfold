use super::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::fmt::Debug;

const ID: &str = "018f0000-0000-7000-8000-000000000001";

fn round_trip<T>(value: Value) -> Result<(), Box<dyn std::error::Error>>
where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    let typed: T = serde_json::from_value(value.clone())?;
    assert_eq!(serde_json::to_value(typed)?, value);
    Ok(())
}

#[test]
fn execute_and_page_requests_use_camel_case_and_strict_bodies()
-> Result<(), Box<dyn std::error::Error>> {
    round_trip::<ChangeExecuteRequest>(json!({"requestVersionId":ID}))?;
    let execute_response = json!({"jobRef":ID,"approvalState":"granted"});
    round_trip::<ChangeExecuteResponse>(execute_response.clone())?;
    let mut future_response = execute_response;
    future_response["futureField"] = json!(true);
    assert!(serde_json::from_value::<ChangeExecuteResponse>(future_response).is_ok());
    round_trip::<ChangeItemResultsRequest>(json!({"jobRef":ID}))?;
    round_trip::<ChangeItemResultsRequest>(json!({
        "jobRef":ID,"resultFilter":"outcome_unknown","cursor":"opaque"
    }))?;
    let null_option: ChangeItemResultsRequest = serde_json::from_value(json!({
        "jobRef":ID,"resultFilter":null,"cursor":null
    }))?;
    assert_eq!(null_option.result_filter, None);
    assert_eq!(null_option.cursor, None);
    assert!(
        serde_json::from_value::<ChangeItemResultsRequest>(json!({
            "jobRef":ID,"pageSize":50
        }))
        .is_err()
    );
    Ok(())
}

#[test]
fn change_result_types_round_trip_normal_and_boundary_values()
-> Result<(), Box<dyn std::error::Error>> {
    round_trip::<ChangeItemCounts>(json!({
        "completed":u32::MAX,"overwritten":0,"skipped":0,"failed":1,"unprocessed":0
    }))?;
    for (state, wire) in [
        (ChangeItemState::Completed, "completed"),
        (ChangeItemState::Skipped, "skipped"),
        (ChangeItemState::Failed, "failed"),
        (ChangeItemState::Unprocessed, "unprocessed"),
    ] {
        assert_eq!(serde_json::to_value(state)?, json!(wire));
        assert_eq!(
            serde_json::from_value::<ChangeItemState>(json!(wire))?,
            state
        );
    }
    for (filter, wire) in [
        (ChangeItemResultFilter::Completed, "completed"),
        (ChangeItemResultFilter::Failed, "failed"),
        (ChangeItemResultFilter::OutcomeUnknown, "outcome_unknown"),
        (ChangeItemResultFilter::Skipped, "skipped"),
        (ChangeItemResultFilter::Held, "held"),
        (ChangeItemResultFilter::Unprocessed, "unprocessed"),
    ] {
        assert_eq!(serde_json::to_value(filter)?, json!(wire));
        assert_eq!(
            serde_json::from_value::<ChangeItemResultFilter>(json!(wire))?,
            filter
        );
    }
    round_trip::<ChangeItemResult>(json!({
        "itemRef":{"resultVersion":ID,"itemNo":u32::MAX},
        "state":"failed","reasonKey":"change.item.outcome_unknown",
        "observedSize":"0","reviewNeeded":true,"held":false,
        "outcomeUnknown":true,"undoAvailable":false
    }))?;
    let response = json!({
        "jobRef":ID,"jobState":"partial",
        "counts":{"completed":1,"overwritten":0,"skipped":u32::MAX,"failed":0,"unprocessed":0},
        "itemRecording":"recorded",
        "items":[{
            "itemRef":{"resultVersion":ID,"itemNo":0},"state":"completed",
            "reviewNeeded":false,"held":false,"outcomeUnknown":false,"undoAvailable":true
        }]
    });
    round_trip::<ChangeItemResultsResponse>(response.clone())?;
    let mut future = response;
    future["futureSummary"] = json!(true);
    future["items"][0]["futureItemField"] = json!("ignored");
    let decoded: ChangeItemResultsResponse = serde_json::from_value(future)?;
    let encoded = serde_json::to_value(decoded)?;
    assert!(encoded.get("futureSummary").is_none());
    assert!(encoded["items"][0].get("futureItemField").is_none());
    assert_eq!(CHANGE_ITEM_IDENTITY_RACE, "change.item.identity_race");
    assert_eq!(CHANGE_ITEM_OUTCOME_UNKNOWN, "change.item.outcome_unknown");
    assert_eq!(
        CHANGE_ITEM_TRASH_UNSUPPORTED,
        "change.item.trash_unsupported"
    );
    assert_eq!(CHANGE_EXECUTE, "change.execute");
    assert_eq!(CHANGE_ITEM_RESULTS, "change.item_results");
    for (reason, key) in [
        (
            CHANGE_ITEM_IDENTITY_UNSUPPORTED,
            "change.item.identity_unsupported",
        ),
        (CHANGE_ITEM_CHANGED, "change.item.changed"),
        (CHANGE_ITEM_IDENTITY_CHANGED, "change.item.identity_changed"),
        (CHANGE_ITEM_CHILDREN_CHANGED, "change.item.children_changed"),
        (
            CHANGE_ITEM_KEEP_COPY_CHANGED,
            "change.item.keep_copy_changed",
        ),
        (CHANGE_ITEM_PLAN_CONFLICT, "change.item.plan_conflict"),
        (
            CHANGE_ITEM_PERMISSION_DENIED,
            "change.item.permission_denied",
        ),
        (CHANGE_ITEM_IDENTITY_RACE, "change.item.identity_race"),
        (CHANGE_ITEM_FILE_IN_USE, "change.item.file_in_use"),
        (CHANGE_ITEM_OS_FAILED, "change.item.os_failed"),
        (
            CHANGE_ITEM_VOLUME_UNAVAILABLE,
            "change.item.volume_unavailable",
        ),
        (CHANGE_ITEM_OUTCOME_UNKNOWN, "change.item.outcome_unknown"),
        (CHANGE_ITEM_CANCELLED, "change.item.cancelled"),
        (
            CHANGE_ITEM_ALREADY_PROCESSED,
            "change.item.already_processed",
        ),
    ] {
        assert_eq!(reason, key);
    }
    Ok(())
}
