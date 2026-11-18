use super::*;
use serde_json::{Value, json};

fn fixture() -> Value {
    let id = "018f0000-0000-7000-8000-000000000001";
    let item = json!({"resultVersion":id,"itemNo":0});
    let qty = json!({"unit":"items","state":"exact","value":"0"});
    let total = json!({
        "quantity":{"unit":"bytes","state":"exact","value":"0"},
        "sourceSize":{"basis":"logical","evidence":"measured","state":"exact","bytes":"0"}
    });
    json!({
        "modelRef":"model", "conditionFingerprint":"condition",
        "aggregateScopeRef":item, "navigationScopeRef":item, "focusItemRef":null,
        "origin":"scan", "staleness":{"state":"current","reasonKey":null},
        "representation":{"format":"treemap","measure":"size","sizeBasis":"logical", "classificationCriterion":"type", "colorCriterion":"type", "denominatorBasis":"scope", "colorMode":"parent_folder", "labelMode":"name_size"},
        "denominator":{"basis":"scope","value":qty},
        "totals":{"scope":total,"drawn":total,"grouped":total,"filteredOut":total},
        "shapes":[{"itemRef":item,"groupRef":null,"matchKey":{"state":"unknown","reasonKey":"unknown"},"geometry":{"primitive":"rect","points":[{"x":0,"y":0},{"x":1,"y":1}]},"size":{"basis":"logical","evidence":"unknown","state":"unknown"},"legendClassKey":"class","displayFlags":[],"shapeRef":"a","parentShapeRef":null,"visibleDepth":0,"rootBranchRef":null}],
        "relationships":[],"legend":[],"summary":{"groupedItems":qty,"omittedItems":qty,"unknownItems":qty,"reasonKeys":[]},
        "strategy":{"id":"treemap","revision":1},"tableRevisions":{"interpretation":1,"requiredData":1,"legend":1,"viewPolicy":1,"sizeRule":{"id":"size","revision":1}}
    })
}

#[test]
fn model_roundtrip_preserves_nullable_hierarchy_and_validates_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let model: VizModel = serde_json::from_value(fixture())?;
    let id = model.aggregate_scope_ref.result_version.clone();
    assert_eq!(model.validate_revision(&id), Ok(()));
    assert_eq!(serde_json::to_value(&model)?, fixture());
    assert!(
        model
            .validate_revision(&Id::from_v7_bytes([2; 16]))
            .is_err()
    );
    Ok(())
}

#[test]
fn rejects_mixed_revisions_and_invalid_shape_graph() -> Result<(), Box<dyn std::error::Error>> {
    let mut value = fixture();
    let id = Id::parse("018f0000-0000-7000-8000-000000000001")?;
    value["shapes"][0]["parentShapeRef"] = json!("a");
    let model: VizModel = serde_json::from_value(value.clone())?;
    assert!(model.validate_revision(&id).is_err());
    value["shapes"][0]["parentShapeRef"] = Value::Null;
    value["shapes"][0]["rootBranchRef"] =
        json!({"resultVersion":"018f0000-0000-7000-8000-000000000002","itemNo":0});
    let model: VizModel = serde_json::from_value(value)?;
    assert!(model.validate_revision(&id).is_err());
    Ok(())
}

#[test]
fn rejects_missing_nullable_fields_and_bad_summary() -> Result<(), Box<dyn std::error::Error>> {
    let mut value = fixture();
    value
        .as_object_mut()
        .ok_or("not object")?
        .remove("focusItemRef");
    assert!(serde_json::from_value::<VizModel>(value).is_err());
    let mut value = fixture();
    value["summary"]["unknownItems"]["unit"] = json!("bytes");
    let model: VizModel = serde_json::from_value(value)?;
    assert!(
        model
            .validate_revision(&model.aggregate_scope_ref.result_version)
            .is_err()
    );
    Ok(())
}

#[test]
fn model_response_and_match_keys_accept_unknown_fields() -> Result<(), Box<dyn std::error::Error>> {
    use crate::visualization::response::VizRenderResponse;
    let id = "018f0000-0000-7000-8000-000000000001";
    let mut model = fixture();
    model["shapes"][0]["matchKey"] = json!({"state":"not_applicable"});
    let response = json!({"kind":"model","requestSeq":1,"resultRevision":id,"observedAt":null,"completeness":"complete","model":model});
    let mut extended = response.clone();
    extended["futureField"] = json!(true);
    extended["model"]["shapes"][0]["matchKey"]["futureField"] = json!(true);
    let parsed: VizRenderResponse = serde_json::from_value(extended)?;
    assert_eq!(serde_json::to_value(parsed)?, response);

    let mut extended = fixture();
    extended["shapes"][0]["matchKey"]["futureField"] = json!(true);
    let parsed: VizModel = serde_json::from_value(extended)?;
    assert_eq!(serde_json::to_value(parsed)?, fixture());
    Ok(())
}

fn bytes_total(bytes: &str) -> Value {
    json!({
        "quantity":{"unit":"bytes","state":"exact","value":bytes},
        "sourceSize":{"basis":"logical","evidence":"measured","state":"exact","bytes":bytes}
    })
}

fn with_remainder() -> Value {
    let mut value = fixture();
    value["totals"] = json!({
        "scope":bytes_total("100"), "drawn":bytes_total("60"), "grouped":bytes_total("30"),
        "filteredOut":bytes_total("0"), "remainder":bytes_total("10"),
        "remainderReasonKeys":["viz.remainder.too_small","viz.remainder.unattributed"]
    });
    value
}

#[test]
fn a_model_without_the_remainder_fields_is_valid_and_serializes_without_them()
-> Result<(), Box<dyn std::error::Error>> {
    let model: VizModel = serde_json::from_value(fixture())?;
    assert!(model.totals.remainder.is_none() && model.totals.remainder_reason_keys.is_none());
    assert_eq!(
        model.validate_revision(&model.aggregate_scope_ref.result_version),
        Ok(())
    );
    let totals = serde_json::to_value(&model)?["totals"].clone();
    assert!(totals.get("remainder").is_none() && totals.get("remainderReasonKeys").is_none());
    Ok(())
}

#[test]
fn a_model_with_a_remainder_roundtrips_and_is_checked_as_a_whole()
-> Result<(), Box<dyn std::error::Error>> {
    let value = with_remainder();
    let model: VizModel = serde_json::from_value(value.clone())?;
    let id = model.aggregate_scope_ref.result_version.clone();
    assert_eq!(model.validate_revision(&id), Ok(()));
    assert_eq!(serde_json::to_value(&model)?, value);

    let mut broken = value.clone();
    broken["totals"]["remainder"] = bytes_total("11");
    let model: VizModel = serde_json::from_value(broken)?;
    assert_eq!(
        model.validate_revision(&id),
        Err("totals do not add up to the scope")
    );

    let mut gap = value.clone();
    gap["totals"]
        .as_object_mut()
        .ok_or("not object")?
        .remove("remainder");
    gap["totals"]
        .as_object_mut()
        .ok_or("not object")?
        .remove("remainderReasonKeys");
    gap["totals"]["drawn"] = bytes_total("80");
    gap["totals"]["grouped"] = bytes_total("30");
    let model: VizModel = serde_json::from_value(gap)?;
    assert_eq!(model.validate_revision(&id), Err("totals exceed the scope"));
    Ok(())
}

#[test]
fn the_remainder_must_match_its_source_size_and_may_be_unknown()
-> Result<(), Box<dyn std::error::Error>> {
    let id = Id::parse("018f0000-0000-7000-8000-000000000001")?;
    let mut other_basis = with_remainder();
    other_basis["totals"]["remainder"]["sourceSize"]["basis"] = json!("allocated");
    let model: VizModel = serde_json::from_value(other_basis)?;
    assert_eq!(
        model.validate_revision(&id),
        Err("model total does not match its source size")
    );

    // An unknown remainder keeps its reason, switches the sum check off and may name its causes.
    let mut unknown = with_remainder();
    unknown["totals"]["remainder"] = json!({
        "quantity":{"unit":"bytes","state":"unknown","reasonKey":"space.data_insufficient"},
        "sourceSize":{"basis":"logical","evidence":"unknown","state":"unknown"}
    });
    unknown["totals"]["remainderReasonKeys"] = json!(["viz.remainder.too_small"]);
    let model: VizModel = serde_json::from_value(unknown.clone())?;
    assert_eq!(model.validate_revision(&id), Ok(()));
    assert_eq!(serde_json::to_value(&model)?, unknown);

    // Cause keys without a remainder are refused.
    let mut orphan = with_remainder();
    orphan["totals"]
        .as_object_mut()
        .ok_or("not object")?
        .remove("remainder");
    let model: VizModel = serde_json::from_value(orphan)?;
    assert_eq!(
        model.validate_revision(&id),
        Err("invalid remainder reason keys")
    );
    Ok(())
}

fn group_shape(count: Value) -> Value {
    let mut shape = fixture()["shapes"][0].clone();
    shape["itemRef"] = Value::Null;
    shape["groupRef"] = json!("g1");
    shape["matchKey"] = json!({"state":"not_applicable"});
    shape["memberCount"] = count;
    shape
}

#[test]
fn a_group_shape_carries_its_member_count_and_an_old_model_is_still_read()
-> Result<(), Box<dyn std::error::Error>> {
    let id = Id::parse("018f0000-0000-7000-8000-000000000001")?;
    // A model from before the field has no member count and writes none back.
    let old: VizModel = serde_json::from_value(fixture())?;
    assert!(old.shapes.iter().all(|shape| shape.member_count.is_none()));
    assert!(
        serde_json::to_value(&old)?["shapes"][0]
            .get("memberCount")
            .is_none()
    );

    for state in [
        json!({"unit":"items","state":"exact","value":"40"}),
        json!({"unit":"items","state":"lower_bound","value":"40"}),
        json!({"unit":"items","state":"unknown","reasonKey":"viz.unavailable.no_attribution"}),
    ] {
        let mut value = fixture();
        value["shapes"] = json!([group_shape(state)]);
        let model: VizModel = serde_json::from_value(value.clone())?;
        assert_eq!(model.validate_revision(&id), Ok(()));
        assert_eq!(serde_json::to_value(&model)?, value);
    }
    Ok(())
}

#[test]
fn the_remainder_fields_are_omitted_not_null() {
    for field in ["remainder", "remainderReasonKeys"] {
        let mut value = with_remainder();
        value["totals"][field] = Value::Null;
        assert!(
            serde_json::from_value::<VizModel>(value).is_err(),
            "{field}"
        );
    }
}

#[test]
fn a_member_count_is_refused_on_an_item_shape_and_outside_items()
-> Result<(), Box<dyn std::error::Error>> {
    let id = Id::parse("018f0000-0000-7000-8000-000000000001")?;
    let exact = json!({"unit":"items","state":"exact","value":"2"});
    let mut item = fixture();
    item["shapes"][0]["memberCount"] = exact;
    let model: VizModel = serde_json::from_value(item)?;
    assert!(model.validate_revision(&id).is_err());

    let mut bytes = fixture();
    bytes["shapes"] = json!([group_shape(
        json!({"unit":"bytes","state":"exact","value":"2"})
    )]);
    let model: VizModel = serde_json::from_value(bytes)?;
    assert!(model.validate_revision(&id).is_err());

    let mut null = fixture();
    null["shapes"] = json!([group_shape(Value::Null)]);
    assert!(serde_json::from_value::<VizModel>(null).is_err());
    Ok(())
}
