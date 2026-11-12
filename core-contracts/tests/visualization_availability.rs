use du_core_contracts::visualization::availability::{
    VizAvailabilityRequest, VizAvailabilityResponse,
};
use serde_json::{Value, from_value, json, to_value};

const REVISION: &str = "018f0000-0000-7000-8000-000000000002";

fn request() -> Value {
    json!({
        "sourceRef":{"kind":"scan","ref":REVISION},
        "resultRevision":REVISION,
        "aggregateScopeRef":{"resultVersion":REVISION,"itemNo":0}
    })
}

#[test]
fn request_requires_explicit_source_revision_and_folder_scope()
-> Result<(), Box<dyn std::error::Error>> {
    let value = request();
    let decoded: VizAvailabilityRequest = from_value(value.clone())?;
    assert_eq!(to_value(decoded)?, value);

    for field in ["sourceRef", "resultRevision", "aggregateScopeRef"] {
        let mut invalid = request();
        invalid
            .as_object_mut()
            .ok_or("expected request object")?
            .remove(field);
        assert!(
            from_value::<VizAvailabilityRequest>(invalid).is_err(),
            "{field}"
        );
    }
    Ok(())
}

#[test]
fn response_keeps_reason_key_required_and_nullable() -> Result<(), Box<dyn std::error::Error>> {
    let value = json!({
        "formats":[
            {"format":"treemap","available":true,"reasonKey":null},
            {"format":"trend","available":false,"reasonKey":"viz.unavailable.single_observation"}
        ],
        "requiredDataTableRevision":1
    });
    let decoded: VizAvailabilityResponse = from_value(value.clone())?;
    assert_eq!(to_value(decoded)?, value);

    let mut missing_reason = value.clone();
    missing_reason["formats"][0]
        .as_object_mut()
        .ok_or("expected format object")?
        .remove("reasonKey");
    assert!(from_value::<VizAvailabilityResponse>(missing_reason).is_err());

    for invalid_format in [
        json!({"format":"treemap","available":true,"reasonKey":"viz.unavailable.no_item_sizes"}),
        json!({"format":"treemap","available":false,"reasonKey":null}),
    ] {
        let mut invalid = value.clone();
        invalid["formats"][0] = invalid_format;
        assert!(from_value::<VizAvailabilityResponse>(invalid).is_err());
    }
    Ok(())
}
