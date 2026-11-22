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

fn ref_json(item_no: u32) -> Value {
    json!({"resultVersion": ID, "itemNo": item_no})
}

fn head_json(readiness: &str) -> Value {
    json!({
        "planVersionId": ID,
        "lineageId": ID,
        "planHash": "a".repeat(64),
        "hashFormat": PLAN_HASH_FORMAT_V1,
        "readiness": readiness,
        "protectionRulesetVersion": "4",
        "protectionClassTableVersion": "8",
        "effectSummary": {},
        "restorationLimitSummary": {},
        "originChannel": "gui",
        "fixedAt": TIME,
        "changeSummary": "initial"
    })
}

#[test]
fn hash_shape_and_boundaries_are_stable() -> Result<(), Box<dyn std::error::Error>> {
    let hash = "0f".repeat(32);
    round_trip::<PlanHash>(json!(hash))?;
    assert!(serde_json::from_value::<PlanHash>(json!("f".repeat(63))).is_err());
    assert!(serde_json::from_value::<PlanHash>(json!("F".repeat(64))).is_err());
    assert!(serde_json::from_value::<PlanHash>(json!("g".repeat(64))).is_err());
    Ok(())
}

#[test]
fn plan_source_and_readiness_values_are_snake_case() {
    for (value, wire) in [
        (PlanSourceKind::Manual, "manual"),
        (PlanSourceKind::GoalAlternative, "goal_alternative"),
        (PlanSourceKind::DuplicateKeep, "duplicate_keep"),
        (PlanSourceKind::Scenario, "scenario"),
    ] {
        assert_eq!(
            serde_json::to_value(value).unwrap_or(Value::Null),
            json!(wire)
        );
    }
    for (value, wire) in [
        (PlanReadiness::ExperiencePlan, "experience_plan"),
        (PlanReadiness::SourceNotCurrent, "source_not_current"),
        (PlanReadiness::NeedsConfirmation, "needs_confirmation"),
        (PlanReadiness::NoExecutableItems, "no_executable_items"),
        (PlanReadiness::ApprovalPossible, "approval_possible"),
    ] {
        assert_eq!(
            serde_json::to_value(value).unwrap_or(Value::Null),
            json!(wire)
        );
    }
}

#[test]
fn draft_commands_round_trip_normal_and_boundary_shapes() -> Result<(), Box<dyn std::error::Error>>
{
    round_trip::<PlanSourceCount>(json!({"source":"manual","count":0}))?;
    round_trip::<PlanDraftSummary>(json!({
        "itemCount":u32::MAX,
        "bySource":[{"source":"scenario","count":u32::MAX}],
        "conversionHeldCount":0
    }))?;
    round_trip::<PlanDraftOpenRequest>(json!({
        "source":"manual",
        "itemRefs":[ref_json(0)]
    }))?;
    let open: PlanDraftOpenRequest = serde_json::from_value(json!({
        "source":"goal_alternative",
        "itemRefs":[],
        "alternativeRef":ID
    }))?;
    assert_eq!(open.alternative_ref.as_ref().map(Id::as_str), Some(ID));
    round_trip::<PlanDraftOpenResponse>(json!({
        "draftRef":ID,
        "editSeq":1,
        "summary":{"itemCount":1,"bySource":[],"conversionHeldCount":0}
    }))?;

    let edits = [
        json!({"kind":"add_item","itemRef":ref_json(0)}),
        json!({"kind":"remove_item","itemRef":ref_json(u32::MAX)}),
        json!({"kind":"set_action","itemRef":ref_json(0),"action":"trash"}),
        json!({"kind":"set_destination","itemRef":ref_json(0),"destination":{
            "encoding":"unix","raw":"L3RtcA","display":"/tmp","lossy":false
        }}),
        json!({"kind":"set_retention_condition","itemRef":ref_json(0),"retentionConditionRef":ID}),
        json!({"kind":"set_overwrite","itemRef":ref_json(0),"overwrite":true}),
    ];
    for edit in edits {
        round_trip::<PlanDraftEdit>(edit)?;
    }
    let cleared: PlanDraftEdit = serde_json::from_value(json!({
        "kind":"set_destination","itemRef":ref_json(0),"destination":null
    }))?;
    assert!(matches!(
        cleared,
        PlanDraftEdit::SetDestination {
            destination: None,
            ..
        }
    ));
    round_trip::<PlanDraftEditRequest>(json!({
        "draftRef":ID,"editSeq":u64::MAX,
        "edit":{"kind":"remove_item","itemRef":ref_json(u32::MAX)}
    }))?;
    round_trip::<PlanDraftEditResponse>(json!({
        "draftRef":ID,"editSeq":1,
        "summary":{"itemCount":0,"bySource":[],"conversionHeldCount":0}
    }))?;
    round_trip::<PlanDraftCancelRequest>(json!({"draftRef":ID}))?;
    round_trip::<PlanDraftCancelResponse>(json!({"discarded":false,"buildCancelled":false}))?;
    round_trip::<PlanBuildRequest>(json!({"draftRef":ID,"editSeq":1}))?;
    round_trip::<PlanBuildResponse>(json!({"jobRef":ID}))?;
    assert!(
        serde_json::from_value::<PlanDraftOpenRequest>(json!({
            "source":"manual","itemRefs":[],"future":true
        }))
        .is_err()
    );
    Ok(())
}

#[test]
fn plan_version_reads_and_events_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    round_trip::<PlanGetRequest>(json!({"planVersionId":ID,"section":"executable"}))?;
    let mut head = head_json("approval_possible");
    head["parentPlanVersionId"] = json!(ID);
    round_trip::<PlanVersionHead>(head)?;
    round_trip::<PlanVersionItem>(json!({
        "itemNo":u32::MAX,
        "itemRef":ref_json(u32::MAX),
        "action":"move",
        "disposition":"held",
        "overwrite":false,
        "expectedIdentity":{"volumeId":[1],"fileId":[2]},
        "protectionSummary":{"state":"allow"},
        "selectionBasis":["manual"],
        "absorbedBy":ref_json(0)
    }))?;
    let mut response = json!({
        "head":head_json("needs_confirmation"),
        "items":[],
        "nextCursor":"next"
    });
    round_trip::<PlanGetResponse>(response.clone())?;
    let mut future_response = response.clone();
    future_response["futureField"] = json!(true);
    assert!(serde_json::from_value::<PlanGetResponse>(future_response).is_ok());
    response["nextCursor"] = Value::Null;
    let parsed: PlanGetResponse = serde_json::from_value(response)?;
    assert!(parsed.next_cursor.is_none());
    round_trip::<PlanItemReasonRequest>(json!({"planVersionId":ID,"itemNo":0}))?;
    round_trip::<PlanItemReasonResponse>(json!({
        "itemRef":ref_json(0),"selectionBasis":[],"restrictions":[],"disposition":"rejected"
    }))?;
    round_trip::<PlanHistoryRequest>(json!({"lineageId":ID}))?;
    round_trip::<PlanHistoryEntry>(json!({
        "planVersionId":ID,"fixedAt":TIME,"changeSummary":"updated"
    }))?;
    round_trip::<PlanHistoryResponse>(json!({"versions":[]}))?;
    round_trip::<PlanVersionFixed>(json!({
        "planVersionId":ID,
        "lineageId":ID,
        "planHash":"b".repeat(64),
        "readiness":"experience_plan",
        "includedCount":0,
        "heldCount":u32::MAX,
        "needsConfirmationCount":0,
        "jobRef":ID,
        "fixedAt":TIME
    }))?;
    let mut future_event =
        serde_json::to_value(serde_json::from_value::<PlanVersionFixed>(json!({
            "planVersionId":ID,"lineageId":ID,"planHash":"c".repeat(64),
            "readiness":"no_executable_items","includedCount":0,"heldCount":0,
            "needsConfirmationCount":0,"jobRef":ID,"fixedAt":TIME
        }))?)?;
    future_event["futureField"] = json!(true);
    assert!(serde_json::from_value::<PlanVersionFixed>(future_event).is_ok());
    assert_eq!(Timestamp::parse(TIME)?.to_string(), TIME);
    for (command, wire) in [
        (PLAN_DRAFT_OPEN, "plan.draft_open"),
        (PLAN_DRAFT_EDIT, "plan.draft_edit"),
        (PLAN_DRAFT_CANCEL, "plan.draft_cancel"),
        (PLAN_BUILD, "plan.build"),
        (PLAN_GET, "plan.get"),
        (PLAN_ITEM_REASON, "plan.item_reason"),
        (PLAN_HISTORY, "plan.history"),
        (PLAN_VERSION_FIXED, "plan.version_fixed"),
        (PLAN_HASH_FORMAT_V1, "du.plan.hash.v1"),
        (PLAN_SOURCE_NOT_CURRENT, "plan.source_not_current"),
        (PLAN_SOURCE_CHANNEL_DENIED, "plan.source_channel_denied"),
        (PLAN_SAMPLE_MIXED_SOURCE, "sample.mixed_source"),
        (PLAN_EMPTY, "plan.empty"),
        (PLAN_TOO_MANY_ITEMS, "plan.too_many_items"),
        (PLAN_RESULT_STALE, "plan.result_stale"),
        (PLAN_DRAFT_STALE, "plan.draft_stale"),
        (PLAN_NOT_FOUND, "plan.not_found"),
        (SESSION_EVICTED, "session.evicted"),
        (PLAN_NOT_READY, "plan.not_ready"),
        (PLAN_STORE_FAILED, "plan.store_failed"),
    ] {
        assert_eq!(command, wire);
    }
    for (reason, key) in [
        (PLAN_ITEM_PROTECTED, "plan.item.protected"),
        (
            PLAN_ITEM_PROTECTED_DESCENDANT,
            "plan.item.protected_descendant",
        ),
        (PLAN_ITEM_UNDETERMINED, "plan.item.undetermined"),
        (PLAN_ITEM_GUIDANCE_ONLY, "plan.item.guidance_only"),
        (PLAN_ITEM_DUPLICATE_TARGET, "plan.item.duplicate_target"),
        (PLAN_ITEM_NESTED_TARGET, "plan.item.nested_target"),
        (PLAN_ITEM_CROSS_VOLUME, "plan.item.cross_volume"),
        (PLAN_ITEM_DEST_EXISTS, "plan.item.dest_exists"),
        (
            PLAN_ITEM_DEST_OVERWRITE_TRASH_UNSUPPORTED,
            "plan.item.dest_overwrite_trash_unsupported",
        ),
        (
            PLAN_ITEM_DEST_OVERWRITE_PROTECTED,
            "plan.item.dest_overwrite_protected",
        ),
        (
            PLAN_ITEM_DEST_OVERWRITE_UNKNOWN,
            "plan.item.dest_overwrite_unknown",
        ),
        (PLAN_ITEM_NEEDS_CONFIRMATION, "plan.item.needs_confirmation"),
        (PLAN_ITEM_OVERLAP_CONFLICT, "plan.item.overlap_conflict"),
        (PLAN_ITEM_DEST_INSIDE_SELF, "plan.item.dest_inside_self"),
        (PLAN_ITEM_DEST_UNKNOWN, "plan.item.dest_unknown"),
        (PLAN_ITEM_ACTION_UNSUPPORTED, "plan.item.action_unsupported"),
        (PLAN_ITEM_KEEP_CONFLICT, "plan.item.keep_conflict"),
        (PLAN_ITEM_SOURCE_MISMATCH, "plan.item.source_mismatch"),
        (PLAN_ITEM_SCENARIO_VIRTUAL, "plan.item.scenario_virtual"),
    ] {
        assert_eq!(reason, key);
    }
    Ok(())
}
