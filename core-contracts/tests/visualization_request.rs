use du_core_contracts::{
    Id,
    visualization::request::{VizRenderRequest, VizViewChannel, VizViewOpenRequest},
};
use serde_json::{Value, from_value, json, to_value};

const VIEW: &str = "018f0000-0000-7000-8000-000000000001";
const REVISION: &str = "018f0000-0000-7000-8000-000000000002";

fn item(number: u32, revision: &str) -> Value {
    json!({"resultVersion": revision, "itemNo": number})
}

fn request() -> Value {
    json!({
        "viewChannel": VIEW,
        "requestSeq": 1,
        "format": "treemap",
        "aggregateScopeRef": item(0, REVISION),
        "navigationScopeRef": item(1, REVISION),
        "measure": "size",
        "sizeBasis": "logical",
        "classificationCriterion": "none",
        "colorCriterion": "folder",
        "denominatorBasis": "scope",
        "viewport": {"width": 640, "height": 460, "scale": 1.25},
        "visibleWindow": {"left": 0, "top": 0, "right": 1_000_000, "bottom": 1_000_000},
        "minAreaPx2": 16,
        "depthLimit": 3
    })
}

#[test]
fn view_channel_and_open_request_are_distinct_bodies() -> Result<(), serde_json::Error> {
    let open: VizViewOpenRequest = from_value(json!({}))?;
    assert_eq!(to_value(open)?, json!({}));
    let channel: VizViewChannel = from_value(json!({"viewChannel": VIEW}))?;
    assert_eq!(to_value(channel)?, json!({"viewChannel": VIEW}));
    assert!(from_value::<VizViewOpenRequest>(json!({"viewChannel": VIEW})).is_err());
    assert!(from_value::<VizViewChannel>(json!({})).is_err());
    Ok(())
}

#[test]
fn omitted_options_round_trip_and_other_revisions_fail_admission()
-> Result<(), Box<dyn std::error::Error>> {
    let value = request();
    let decoded: VizRenderRequest = from_value(value.clone())?;
    assert_eq!(to_value(&decoded)?, value);
    decoded.validate_revision(&Id::parse(REVISION)?)?;
    assert!(decoded.validate_revision(&Id::parse(VIEW)?).is_err());

    let mut value = request();
    value["focusItemRef"] = item(2, VIEW);
    let mismatched: VizRenderRequest = from_value(value)?;
    assert!(mismatched.validate_revision(&Id::parse(REVISION)?).is_err());
    Ok(())
}

#[test]
fn wire_rejects_out_of_bounds_and_unrecognized_fields() {
    for (field, invalid) in [
        ("requestSeq", json!(0)),
        ("minAreaPx2", json!(3)),
        ("minAreaPx2", json!(257)),
        ("viewport", json!({"width": 0, "height": 1, "scale": 1})),
        ("viewport", json!({"width": 1, "height": 1, "scale": -1})),
        (
            "visibleWindow",
            json!({"left": 0, "top": 0, "right": 1_000_001, "bottom": 1}),
        ),
        (
            "visibleWindow",
            json!({"left": 1, "top": 0, "right": 1, "bottom": 1}),
        ),
        ("sourceRef", Value::Null),
        ("colorMode", Value::Null),
        ("expandedDepthByItem", Value::Null),
        ("correlationId", json!(VIEW)),
        (
            "condition",
            json!({"kind":"query_result","queryRef":VIEW,"condition":{"kind":"all","children":[]}}),
        ),
    ] {
        let mut value = request();
        value[field] = invalid;
        assert!(from_value::<VizRenderRequest>(value).is_err(), "{field}");
    }
}

#[test]
fn expansion_wire_preserves_entries_for_engine_policy_validation() -> Result<(), serde_json::Error>
{
    let mut value = request();
    let expanded: Vec<_> = (0..64)
        .map(|number| json!({"itemRef": item(number, REVISION), "extraDepth": 8}))
        .collect();
    value["expandedDepthByItem"] = json!(expanded);
    let decoded: VizRenderRequest = from_value(value.clone())?;
    assert_eq!(to_value(decoded)?, value);

    let mut over_count = expanded.clone();
    over_count.push(json!({"itemRef": item(64, REVISION), "extraDepth": 8}));
    for entries in [
        json!(over_count),
        json!([expanded[0], expanded[0]]),
        json!([{"itemRef": item(0, REVISION), "extraDepth": 9}]),
    ] {
        value["expandedDepthByItem"] = entries;
        assert_eq!(
            to_value(from_value::<VizRenderRequest>(value.clone())?)?,
            value
        );
    }
    Ok(())
}
