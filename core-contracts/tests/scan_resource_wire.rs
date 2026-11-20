use du_core_contracts::{
    PathBytes,
    analysis_ipc::{
        ScanCollectionCondition, ScanLaneStatus, ScanResourceMode, ScanSetResourceResponse,
        ScanStart, ScanStartRequest,
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
        "lanes": [{ "laneIndex": 0, "effectiveLimit": 2, "permit": 16 }],
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
            "mode": "quiet",
            "permit": 16,
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
