use du_core_contracts::visualization::locate::{VizLocate, VizLocateRequest};
use serde_json::{from_value, json, to_value};

#[test]
fn locate_request_requires_a_model_and_one_versioned_item() -> Result<(), serde_json::Error> {
    let mut valid = json!({
        "modelRef": "model-1",
        "itemRef": { "resultVersion": "018f0000-0000-7000-8000-000000000001", "itemNo": 0 }
    });
    let request: VizLocateRequest = from_value(valid.clone())?;
    assert_eq!(to_value(request)?, valid);
    valid["modelRef"] = json!("");
    assert!(from_value::<VizLocateRequest>(valid.clone()).is_err());
    valid["modelRef"] = json!("model-1");
    valid["futureField"] = json!(true);
    assert!(from_value::<VizLocateRequest>(valid).is_err());
    assert!(from_value::<VizLocateRequest>(json!({ "modelRef": "model-1" })).is_err());
    Ok(())
}

#[test]
fn locate_variants_preserve_only_the_fields_for_their_location() -> Result<(), serde_json::Error> {
    for (wire, response) in [
        (
            json!({"location":"shape","shapeIndex":0}),
            VizLocate::Shape { shape_index: 0 },
        ),
        (
            json!({"location":"group","groupRef":"group-1"}),
            VizLocate::Group {
                group_ref: "group-1".into(),
            },
        ),
        (
            json!({"location":"outside_view"}),
            VizLocate::OutsideView {},
        ),
        (
            json!({"location":"outside_scope"}),
            VizLocate::OutsideScope {},
        ),
    ] {
        assert_eq!(from_value::<VizLocate>(wire.clone())?, response);
        assert_eq!(to_value(response)?, wire);
    }
    for bad in [
        json!({"location":"shape"}),
        json!({"location":"shape","shapeIndex":-1}),
        json!({"location":"group","groupRef":""}),
    ] {
        assert!(from_value::<VizLocate>(bad.clone()).is_err(), "{bad}");
    }
    Ok(())
}

#[test]
fn locate_response_accepts_unknown_fields() -> Result<(), serde_json::Error> {
    // The location tag selects the variant; another location's field is an unknown field.
    for (extended, response) in [
        (
            json!({"location":"outside_scope","shapeIndex":1}),
            json!({"location":"outside_scope"}),
        ),
        (
            json!({"location":"shape","shapeIndex":1,"groupRef":"wrong"}),
            json!({"location":"shape","shapeIndex":1}),
        ),
        (
            json!({"location":"group","groupRef":"group-1","futureField":true}),
            json!({"location":"group","groupRef":"group-1"}),
        ),
    ] {
        assert_eq!(to_value(from_value::<VizLocate>(extended)?)?, response);
    }
    Ok(())
}
