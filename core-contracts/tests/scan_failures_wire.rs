use du_core_contracts::{
    Id,
    scan_failures::{
        ScanFailureKind as Kind, ScanFailureStage as Stage, ScanFailuresRequest, unentered_reason,
    },
};
use serde_json::{Value, json};

fn body(root_indexes: Value, failures_only: bool) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(json!({
        "resultVersion": Id::parse("018f0000-0000-7000-8000-000000000001")?,
        "rootIndexes": root_indexes,
        "failuresOnly": failures_only
    }))
}

#[test]
fn scan_failures_request_round_trips_optional_filters() -> Result<(), Box<dyn std::error::Error>> {
    let value = body(json!([0, 2, 9]), true)?;
    let request: ScanFailuresRequest = serde_json::from_value(value.clone())?;
    assert_eq!(serde_json::to_value(request)?, value);

    let omitted: ScanFailuresRequest = serde_json::from_value(json!({
        "resultVersion": "018f0000-0000-7000-8000-000000000001"
    }))?;
    assert_eq!(omitted.root_indexes, None);
    assert_eq!(omitted.failures_only, None);
    assert_eq!(
        serde_json::to_value(omitted)?,
        json!({ "resultVersion": "018f0000-0000-7000-8000-000000000001" })
    );
    Ok(())
}

#[test]
fn malformed_root_indexes_are_rejected_during_wire_deserialization()
-> Result<(), Box<dyn std::error::Error>> {
    let invalid = [
        Value::Null,
        json!([]),
        json!((0..65).collect::<Vec<u32>>()),
        json!([0, 2, 1]),
        json!([0, 0]),
        json!([-1]),
        json!([4_294_967_296_u64]),
        json!(["0"]),
        json!(0),
    ];
    for indexes in invalid {
        assert!(serde_json::from_value::<ScanFailuresRequest>(body(indexes, false)?).is_err());
    }
    assert!(
        serde_json::from_value::<ScanFailuresRequest>(json!({
            "resultVersion": "018f0000-0000-7000-8000-000000000001",
            "futureField": true
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<ScanFailuresRequest>(json!({
            "resultVersion": "018f0000-0000-7000-8000-000000000001",
            "failuresOnly": null
        }))
        .is_err()
    );
    Ok(())
}

#[test]
fn lane_failure_classification_matches_recorded_failure_rows() {
    assert!(Kind::CollectionFailed.counts_as_lane_failure(Stage::Metadata, "scan.path_denied"));
    for stage in [Stage::OpenDirectory, Stage::Enumerate] {
        assert!(Kind::UnreadRange.counts_as_lane_failure(stage, "scan.path_denied"));
    }
    assert!(
        Kind::UnreadRange
            .counts_as_lane_failure(Stage::Traverse, unentered_reason::BOUNDARY_UNVERIFIABLE)
    );
    assert!(Kind::UnreadRange.counts_as_lane_failure(Stage::Traverse, "scan.unclassified"));
    assert!(
        !Kind::UnreadRange
            .counts_as_lane_failure(Stage::Traverse, unentered_reason::BOUNDARY_OTHER_VOLUME)
    );
    assert!(
        !Kind::UnreadRange
            .counts_as_lane_failure(Stage::Traverse, unentered_reason::REVISIT_BLOCKED)
    );
    assert!(!Kind::Uncertain.counts_as_lane_failure(Stage::Metadata, "scan.item_unverifiable"));
    assert!(
        !Kind::UntraversedLink.counts_as_lane_failure(Stage::Traverse, "scan.link_untraversed")
    );
}
