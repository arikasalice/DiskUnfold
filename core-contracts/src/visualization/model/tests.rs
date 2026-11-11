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
