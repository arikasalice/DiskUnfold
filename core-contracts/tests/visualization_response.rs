use du_core_contracts::visualization::response::VizRenderResponse;
use serde_json::{Value, from_value, json, to_value};

const REVISION: &str = "018f0000-0000-7000-8000-000000000002";
const OTHER: &str = "018f0000-0000-7000-8000-000000000003";

fn rendered_model() -> Value {
    let root = json!({"resultVersion": REVISION, "itemNo": 0});
    let quantity = json!({"unit":"bytes", "state":"exact", "value":"0"});
    let size = json!({"basis":"logical", "evidence":"measured", "state":"exact", "bytes":"0"});
    let total = json!({"quantity":quantity, "sourceSize":size});
    let zero_items = json!({"unit":"items", "state":"exact", "value":"0"});
    json!({
        "kind":"model", "requestSeq":1, "resultRevision":REVISION,
        "observedAt":null, "completeness":"complete",
        "model":{
            "modelRef":"m1", "conditionFingerprint":"c1",
            "aggregateScopeRef":root, "navigationScopeRef":root, "focusItemRef":null,
            "origin":"current_analysis", "staleness":{"state":"current", "reasonKey":null},
            "representation":{
                "format":"treemap", "measure":"size", "sizeBasis":"logical",
                "classificationCriterion":"none", "colorCriterion":"folder",
                "denominatorBasis":"scope", "colorMode":"parent_folder", "labelMode":"name_size"
            },
            "denominator":{"basis":"scope", "value":quantity, "size":size},
            "totals":{"scope":total,"drawn":total,"grouped":total,"filteredOut":total},
            "shapes":[], "relationships":[], "legend":[],
            "summary":{
                "groupedItems":zero_items,"omittedItems":zero_items,
                "unknownItems":zero_items,"reasonKeys":[]
            },
            "strategy":{"id":"viz.treemap", "revision":1},
            "tableRevisions":{
                "interpretation":1, "requiredData":1, "legend":1, "viewPolicy":1,
                "sizeRule":{"id":"size.logical","revision":1}
            }
        }
    })
}

#[test]
fn model_response_preserves_the_empty_scope_and_its_original_size() -> Result<(), serde_json::Error>
{
    let value = rendered_model();
    let response: VizRenderResponse = from_value(value.clone())?;
    assert_eq!(to_value(response)?, value);
    Ok(())
}

#[test]
fn model_response_rejects_stale_references_and_branch_pollution() {
    for field in ["aggregateScopeRef", "navigationScopeRef"] {
        let mut value = rendered_model();
        value["model"][field]["resultVersion"] = json!(OTHER);
        assert!(from_value::<VizRenderResponse>(value).is_err(), "{field}");
    }
    let mut value = rendered_model();
    value["jobRef"] = json!(OTHER);
    assert!(from_value::<VizRenderResponse>(value).is_err());
    let mut value = rendered_model();
    if let Some(object) = value.as_object_mut() {
        object.remove("observedAt");
    }
    assert!(from_value::<VizRenderResponse>(value).is_err());
}

#[test]
fn size_total_requires_matching_source_evidence() {
    for invalid in [
        Value::Null,
        json!({"basis":"allocated","evidence":"measured","state":"exact","bytes":"0"}),
        json!({"basis":"logical","evidence":"measured","state":"lower_bound","bytes":"0"}),
        json!({"basis":"logical","evidence":"measured","state":"exact","bytes":"1"}),
    ] {
        let mut value = rendered_model();
        value["model"]["totals"]["scope"]["sourceSize"] = invalid;
        assert!(from_value::<VizRenderResponse>(value).is_err());
    }
}

#[test]
fn non_model_branches_keep_their_existing_wire_shapes() -> Result<(), serde_json::Error> {
    for value in [
        json!({
            "kind":"job", "requestSeq":2, "resultRevision":REVISION, "observedAt":null,
            "completeness":"partial", "jobRef":OTHER
        }),
        json!({
            "kind":"unavailable", "requestSeq":2, "resultRevision":REVISION, "observedAt":null,
            "completeness":"partial", "reasonKey":"viz.format_unavailable",
            "availableFormats":["treemap"], "fallback":{"modelRef":"m1","resultRevision":REVISION}
        }),
    ] {
        let response: VizRenderResponse = from_value(value.clone())?;
        assert_eq!(to_value(response)?, value);
        let mut invalid = value;
        invalid["requestSeq"] = json!(0);
        assert!(from_value::<VizRenderResponse>(invalid).is_err());
    }
    Ok(())
}
