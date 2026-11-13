use du_core_contracts::visualization::render::VizRenderNonModelResponse;
use serde_json::{Value, json};

#[test]
fn specified_render_branches_round_trip_and_keep_required_nulls() -> Result<(), String> {
    let revision = "018f0000-0000-7000-8000-000000000001";
    let job = json!({"kind":"job", "requestSeq":1, "resultRevision":revision,
        "observedAt":null, "completeness":"partial", "jobRef":"018f0000-0000-7000-8000-000000000002"});
    let unavailable = json!({"kind":"unavailable", "requestSeq":2, "resultRevision":revision,
        "observedAt":null, "completeness":"partial", "reasonKey":"viz.unavailable.no_attribution",
        "availableFormats":["bar"], "fallback":{"modelRef":"model", "resultRevision":revision}});
    for body in [job, unavailable] {
        let value: VizRenderNonModelResponse =
            serde_json::from_value(body.clone()).map_err(|e| e.to_string())?;
        assert_eq!(
            serde_json::to_value(value).map_err(|e| e.to_string())?,
            body
        );
    }
    Ok(())
}

#[test]
fn render_branches_reject_missing_fields_wrong_branch_and_mismatched_fallback() {
    let revision = "018f0000-0000-7000-8000-000000000001";
    let base = json!({"kind":"unavailable", "requestSeq":1, "resultRevision":revision,
        "observedAt":null, "completeness":"partial", "reasonKey":"viz.unavailable.no_attribution",
        "availableFormats":[], "fallback":null});
    for key in ["observedAt", "fallback", "availableFormats", "reasonKey"] {
        let mut body = base.clone();
        if let Value::Object(fields) = &mut body {
            fields.remove(key);
        }
        assert!(serde_json::from_value::<VizRenderNonModelResponse>(body).is_err());
    }
    let mut mixed = base.clone();
    mixed["jobRef"] = json!("018f0000-0000-7000-8000-000000000002");
    assert!(serde_json::from_value::<VizRenderNonModelResponse>(mixed).is_err());
    let mut wrong = base.clone();
    wrong["fallback"] =
        json!({"modelRef":"model", "resultRevision":"018f0000-0000-7000-8000-000000000002"});
    assert!(serde_json::from_value::<VizRenderNonModelResponse>(wrong).is_err());
    let mut model = base;
    model["kind"] = json!("model");
    assert!(serde_json::from_value::<VizRenderNonModelResponse>(model).is_err());
    let job = json!({"kind":"job", "requestSeq":1, "resultRevision":revision,
        "completeness":"partial", "jobRef":"018f0000-0000-7000-8000-000000000002"});
    assert!(serde_json::from_value::<VizRenderNonModelResponse>(job).is_err());
}
