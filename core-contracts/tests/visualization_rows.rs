use du_core_contracts::visualization::rows::{
    VizGroupRowsRequest, VizRatio, VizRow, VizRows, VizRowsRequest,
};
use serde_json::{Value, json};
const ID: &str = "018f0000-0000-7000-8000-000000000001";
const OTHER: &str = "018f0000-0000-7000-8000-000000000002";
fn row() -> Value {
    json!({"itemRef":{"resultVersion":ID,"itemNo":1},"groupRef":null,"displayName":"a","size":{"basis":"logical","evidence":"measured","state":"exact","bytes":"20"},"ratio":"-0.25","denominator":{"basis":"scope","value":{"unit":"bytes","state":"exact","value":"80"}},"legendClassKey":"future.class","displayFlags":["future.flag"]})
}
#[test]
fn preserves_rows_quantities_and_nullable_fields() -> Result<(), Box<dyn std::error::Error>> {
    for quantity in [
        None,
        Some(json!({"unit":"items","state":"exact","value":"2"})),
        Some(json!({"unit":"bytes","state":"exact","value":"-5"})),
        Some(json!({"unit":"bytes","state":"unknown","reasonKey":"space.data_insufficient"})),
    ] {
        let mut value = row();
        if let Some(q) = quantity {
            value["quantity"] = q;
        }
        let decoded: VizRow = serde_json::from_value(value.clone())?;
        assert_eq!(serde_json::to_value(decoded)?, value);
    }
    let mut group = row();
    group["itemRef"] = Value::Null;
    group["groupRef"] = json!("g1");
    group["ratio"] = Value::Null;
    group["denominator"]["size"] =
        json!({"basis":"logical","evidence":"unknown","state":"unknown"});
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<VizRow>(group.clone())?)?,
        group
    );
    Ok(())
}
#[test]
fn rejects_bad_refs_flags_and_missing_or_null_fields() {
    let mut cases = Vec::new();
    let mut v = row();
    v["groupRef"] = json!("g");
    cases.push(v);
    let mut v = row();
    v["itemRef"] = Value::Null;
    cases.push(v);
    let mut v = row();
    v["itemRef"] = Value::Null;
    v["groupRef"] = json!("");
    cases.push(v);
    let mut v = row();
    v["displayFlags"] = json!(["x", "x"]);
    cases.push(v);
    let mut v = row();
    v["displayFlags"] = json!([""]);
    cases.push(v);
    for field in ["itemRef", "groupRef", "ratio"] {
        let mut v = row();
        v.as_object_mut().map(|o| o.remove(field));
        cases.push(v);
    }
    let mut v = row();
    v["quantity"] = Value::Null;
    cases.push(v);
    let mut v = row();
    v["denominator"]["size"] = Value::Null;
    cases.push(v);
    for v in cases {
        assert!(serde_json::from_value::<VizRow>(v.clone()).is_err(), "{v}");
    }
}
#[test]
fn canonical_ratio_has_no_lossy_or_ambiguous_representation() {
    for v in [
        "0",
        "-1",
        "0.00000000000000000001",
        "-0.5",
        "18446744073709551615",
    ] {
        assert!(VizRatio::parse(v).is_ok(), "{v}");
    }
    for v in [
        "",
        "-0",
        "+1",
        "01",
        "1.",
        ".1",
        "1.0",
        "-0.0",
        "1e2",
        "0.000000000000000000001",
        "1.2.3",
    ] {
        assert!(VizRatio::parse(v).is_err(), "{v}");
    }
    assert!(serde_json::from_value::<VizRatio>(json!(0.5)).is_err());
}
#[test]
fn pages_check_ranges_and_revision_but_leave_model_membership_to_engine()
-> Result<(), Box<dyn std::error::Error>> {
    let request = json!({"modelRef":"m","resultRevision":ID,"start":0,"count":1000});
    let parsed: VizRowsRequest = serde_json::from_value(request.clone())?;
    parsed.validate()?;
    assert_eq!(serde_json::to_value(parsed)?, request);
    for count in [0_u64, 1001, u64::from(u32::MAX) + 1] {
        let mut v = request.clone();
        v["count"] = json!(count);
        assert!(serde_json::from_value::<VizRowsRequest>(v).is_err());
    }
    let page = json!({"rows":[row()],"totalCount":1,"resultRevision":ID,"observedAt":null,"completeness":"complete"});
    let parsed: VizRows = serde_json::from_value(page.clone())?;
    parsed.validate()?;
    assert_eq!(serde_json::to_value(parsed)?, page);
    let mut wrong = page.clone();
    wrong["resultRevision"] = json!(OTHER);
    assert!(serde_json::from_value::<VizRows>(wrong).is_err());
    let mut too_small = page.clone();
    too_small["totalCount"] = json!(0);
    assert!(serde_json::from_value::<VizRows>(too_small).is_err());
    for field in ["observedAt", "completeness"] {
        let mut missing = page.clone();
        missing.as_object_mut().map(|o| o.remove(field));
        assert!(serde_json::from_value::<VizRows>(missing).is_err());
    }
    let mut known = page.clone();
    known["observedAt"] = json!("2026-11-10T00:00:00.000Z");
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<VizRows>(known.clone())?)?,
        known
    );
    let mut unsafe_count = page;
    unsafe_count["totalCount"] = json!(9007199254740992_u64);
    assert!(serde_json::from_value::<VizRows>(unsafe_count).is_err());
    Ok(())
}

#[cfg(feature = "typescript")]
#[test]
fn generated_types_match_wire_declarations() {
    use du_core_contracts::visualization::rows::{VizDenominator, VizDenominatorBasis};
    use ts_rs::TS;
    let generated = include_str!("../../gui-web/src/ipc/generated/contracts.ts");
    let config = ts_rs::Config::default();
    for declaration in [
        VizRatio::decl(&config),
        VizDenominatorBasis::decl(&config),
        VizDenominator::decl(&config),
        VizRow::decl(&config),
        VizRowsRequest::decl(&config),
        VizGroupRowsRequest::decl(&config),
        VizRows::decl(&config),
    ] {
        let normalized = declaration
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            generated.contains(&format!("export {normalized}\n")),
            "missing {normalized}"
        );
    }
}

#[test]
fn group_rows_request_has_published_fields_and_rejects_invalid_values()
-> Result<(), Box<dyn std::error::Error>> {
    let input = serde_json::json!({"modelRef":"model", "groupRef":"group", "start":0, "count":1});
    let request: VizGroupRowsRequest = serde_json::from_value(input.clone())?;
    request.validate()?;
    assert_eq!(serde_json::to_value(&request)?, input);
    let max = serde_json::json!({"modelRef":"model", "groupRef":"group", "start":u32::MAX, "count":u32::MAX});
    let max_request: VizGroupRowsRequest = serde_json::from_value(max.clone())?;
    max_request.validate()?;
    assert_eq!(serde_json::to_value(max_request)?, max);
    for invalid in [
        serde_json::json!({"groupRef":"group", "start":0, "count":1}),
        serde_json::json!({"modelRef":"model", "start":0, "count":1}),
        serde_json::json!({"modelRef":"model", "groupRef":"group", "count":1}),
        serde_json::json!({"modelRef":"model", "groupRef":"group", "start":0}),
        serde_json::json!({"modelRef":"model", "groupRef":"group", "start":4294967296_u64, "count":1}),
        serde_json::json!({"modelRef":"model", "groupRef":"group", "start":0, "count":4294967296_u64}),
        serde_json::json!({"modelRef":"model", "groupRef":"group", "start":0, "count":1.5}),
        serde_json::json!({"modelRef":"", "groupRef":"group", "start":0, "count":1}),
        serde_json::json!({"modelRef":"model", "groupRef":"", "start":0, "count":1}),
        serde_json::json!({"modelRef":"model", "groupRef":"group", "start":0, "count":0}),
        serde_json::json!({"modelRef":"model", "groupRef":"group", "start":-1, "count":1}),
        serde_json::json!({"modelRef":"model", "groupRef":"group", "start":0, "count":1, "resultRevision":"extra"}),
    ] {
        assert!(serde_json::from_value::<VizGroupRowsRequest>(invalid).is_err());
    }
    Ok(())
}
