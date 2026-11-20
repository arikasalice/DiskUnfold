use du_core_contracts::{
    PathBytes,
    analysis_ipc::{
        DeviceKind, DeviceKindSource, ScanCollectionCondition, ScanLaneStatus, ScanResourceMode,
        ScanSetResourceResponse, ScanStart, ScanStartRequest,
    },
};
use serde_json::{Value, from_value, json, to_value};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn request(collection: Option<Value>) -> Value {
    let mut body = json!({
        "draft": {
            "includes": [{
                "encoding": "unix",
                "raw": PathBytes::from_bytes(b"/data"),
                "display": "/data",
                "lossy": false
            }],
            "exclusions": [],
            "boundary": "current_volume",
            "includedMounts": [],
            "source": "gui"
        },
        "reviewFingerprint": "scope"
    });
    if let Some(collection) = collection {
        body["collection"] = collection;
    }
    body
}

#[test]
fn scan_collection_request_and_receipt_round_trip_with_and_without_condition() -> TestResult {
    for condition in [
        None,
        Some(json!({})),
        Some(json!({
            "resourceMode": "quiet",
            "userLimit": 2
        })),
    ] {
        let value = request(condition);
        let decoded: ScanStartRequest = from_value(value.clone())?;
        assert_eq!(to_value(decoded)?, value);
    }

    let omitted: ScanStartRequest = from_value(request(None))?;
    assert_eq!(omitted.collection, None);
    let defaulted: ScanCollectionCondition = from_value(json!({"userLimit": 2}))?;
    assert_eq!(defaulted.resource_mode, None);
    assert_eq!(defaulted.user_limit, Some(2));

    let receipt = json!({
        "jobRef": "0195f856-ab83-7a59-9343-10d29775c9a9",
        "existing": false,
        "scopeVersion": "0195f856-ab83-7a59-9343-10d29775c9a8",
        "scopeFingerprint": "scope",
        "collection": { "resourceMode": "auto" }
    });
    let decoded: ScanStart = from_value(receipt.clone())?;
    assert_eq!(to_value(decoded)?, receipt);
    Ok(())
}

#[test]
fn unknown_resource_mode_is_rejected() {
    assert!(
        from_value::<ScanStartRequest>(request(Some(json!({
            "resourceMode": "adaptive"
        }))))
        .is_err()
    );
    assert!(from_value::<ScanResourceMode>(json!("adaptive")).is_err());
    assert!(
        from_value::<ScanStartRequest>(request(Some(json!({
            "resourceMode": "auto",
            "futureField": true
        }))))
        .is_err()
    );
}

#[test]
fn applies_from_and_strategy_id_remain_open_strings() -> TestResult {
    let response = json!({
        "outcome": "accepted",
        "lanes": [{ "laneIndex": 0, "effectiveLimit": 2, "permit": 16, "share": 2 }],
        "appliesFrom": "future_assignment_boundary"
    });
    let decoded: ScanSetResourceResponse = from_value(response.clone())?;
    assert_eq!(to_value(decoded)?, response);

    let status = json!({
        "sequence": 1,
        "lanes": [{
            "laneIndex": 0,
            "rootIndexes": [0],
            "inFlight": 1,
            "limit": 2,
            "deviceKind": "ssd",
            "deviceKindSource": "detected",
            "mode": "quiet",
            "permit": 16,
            "share": 2,
            "strategy": { "id": "future.dynamic", "version": 99 },
            "waitReason": "none",
            "waitConfirmed": true,
            "remainingDirectories": "0",
            "failures": "0"
        }],
        "observedAt": "2026-11-20T00:00:00.000Z"
    });
    let decoded: ScanLaneStatus = from_value(status.clone())?;
    assert_eq!(to_value(decoded)?, status);
    Ok(())
}

#[test]
fn lane_device_values_round_trip_and_omitted_values_stay_unknown() -> TestResult {
    let response: ScanSetResourceResponse = from_value(json!({
        "outcome": "accepted",
        "lanes": [{"laneIndex": 0, "effectiveLimit": 1, "permit": 4}],
        "appliesFrom": "next_assignment"
    }))?;
    let lane = response.lanes.first().ok_or("missing resource lane")?;
    assert_eq!(lane.share, None);

    let omitted: ScanLaneStatus = from_value(json!({
        "sequence": 1,
        "lanes": [{
            "laneIndex": 0, "rootIndexes": [0], "inFlight": 0, "limit": 1,
            "waitReason": "none", "waitConfirmed": false,
            "remainingDirectories": "0", "failures": "0"
        }],
        "observedAt": "2026-11-20T00:00:00.000Z"
    }))?;
    let lane = omitted.lanes.first().ok_or("missing status lane")?;
    assert_eq!(lane.device_kind, None);
    assert_eq!(lane.device_kind_source, None);
    assert_eq!(lane.share, None);
    Ok(())
}

#[test]
fn unknown_device_enum_values_are_preserved_as_unknown() -> TestResult {
    let status: ScanLaneStatus = from_value(json!({
        "sequence": 1,
        "lanes": [{
            "laneIndex": 0, "rootIndexes": [0], "inFlight": 0, "limit": 1,
            "deviceKind": "hybrid", "deviceKindSource": "firmware",
            "waitReason": "none", "waitConfirmed": false,
            "remainingDirectories": "0", "failures": "0"
        }],
        "observedAt": "2026-11-20T00:00:00.000Z"
    }))?;
    let lane = status.lanes.first().ok_or("missing status lane")?;
    assert_eq!(lane.device_kind, Some(DeviceKind::Unknown));
    assert_eq!(lane.device_kind_source, Some(DeviceKindSource::Unknown));
    assert!(
        from_value::<ScanSetResourceResponse>(json!({
            "outcome": "accepted", "lanes": [{
                "laneIndex": 0, "effectiveLimit": 1, "share": 0
            }], "appliesFrom": "next_assignment"
        }))
        .is_err()
    );
    Ok(())
}

#[test]
fn serialized_lane_and_resource_responses_never_forward_disk_keys() -> TestResult {
    let secret = "opaque-physical-disk-key";
    let status: ScanLaneStatus = from_value(json!({
        "sequence": 1,
        "lanes": [{
            "laneIndex": 0, "rootIndexes": [0], "inFlight": 0, "limit": 2,
            "deviceKind": "hdd", "deviceKindSource": "detected",
            "permit": 4, "share": 2, "diskKey": secret,
            "waitReason": "none", "waitConfirmed": false,
            "remainingDirectories": "0", "failures": "0"
        }],
        "observedAt": "2026-11-20T00:00:00.000Z"
    }))?;
    let resource: ScanSetResourceResponse = from_value(json!({
        "outcome": "accepted",
        "lanes": [{
            "laneIndex": 0, "effectiveLimit": 2,
            "permit": 4, "share": 2, "diskKey": secret
        }],
        "appliesFrom": "next_assignment"
    }))?;
    let serialized = format!("{}{}", to_value(status)?, to_value(resource)?);
    assert!(!serialized.contains(secret));
    Ok(())
}
