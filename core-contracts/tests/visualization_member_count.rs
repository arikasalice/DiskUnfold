//! The optional member count of a group row and the optional folder reference of a rows
//! request. Both are additions to the rows contract: a response or request from before
//! them is still read and written back unchanged.
use du_core_contracts::visualization::rows::{VizRow, VizRows, VizRowsRequest};
use serde_json::{Value, json};
const ID: &str = "018f0000-0000-7000-8000-000000000001";
fn item_row() -> Value {
    json!({"itemRef":{"resultVersion":ID,"itemNo":1},"groupRef":null,"displayName":"a","size":{"basis":"logical","evidence":"measured","state":"exact","bytes":"20"},"ratio":"0.25","denominator":{"basis":"scope","value":{"unit":"bytes","state":"exact","value":"80"}},"legendClassKey":"class","displayFlags":[]})
}
fn group_row() -> Value {
    let mut group = item_row();
    group["itemRef"] = Value::Null;
    group["groupRef"] = json!("g1");
    group
}
fn count(state: Value) -> Value {
    let mut quantity = json!({"unit":"items"});
    if let (Some(target), Some(source)) = (quantity.as_object_mut(), state.as_object()) {
        target.extend(source.clone());
    }
    quantity
}
fn request() -> Value {
    json!({"modelRef":"m","resultRevision":ID,"start":0,"count":10})
}

#[test]
fn a_group_row_carries_its_member_count_and_an_old_row_is_still_read()
-> Result<(), Box<dyn std::error::Error>> {
    // A row from before the field has none and writes none back, not even a null.
    let old: VizRow = serde_json::from_value(group_row())?;
    assert_eq!(old.member_count, None);
    assert!(serde_json::to_value(&old)?.get("memberCount").is_none());

    let mut group = group_row();
    group["memberCount"] = count(json!({"state":"exact","value":"40"}));
    let decoded: VizRow = serde_json::from_value(group.clone())?;
    assert_eq!(
        decoded.member_count.as_ref().map(|count| count.unit()),
        Some("items")
    );
    assert_eq!(serde_json::to_value(decoded)?, group);
    // The count stays in a page of rows and survives the page check and a round trip.
    let page = json!({
        "rows": [group],
        "totalCount": 1,
        "resultRevision": ID,
        "observedAt": null,
        "completeness": "complete"
    });
    let rows: VizRows = serde_json::from_value(page.clone())?;
    assert_eq!(serde_json::to_value(rows)?, page);
    Ok(())
}

#[test]
fn a_lower_bound_or_unknown_member_count_is_kept_and_never_turned_into_zero()
-> Result<(), Box<dyn std::error::Error>> {
    for state in [
        json!({"state":"lower_bound","value":"3"}),
        json!({"state":"unknown","reasonKey":"viz.unavailable.no_attribution"}),
        json!({"state":"exact","value":"0"}),
    ] {
        let mut group = group_row();
        group["memberCount"] = count(state);
        let decoded: VizRow = serde_json::from_value(group.clone())?;
        assert_eq!(serde_json::to_value(decoded)?, group);
    }
    // An unknown count has to say why, and no value comes with it.
    for bad in [
        json!({"state":"unknown"}),
        json!({"state":"unknown","reasonKey":"k","value":"0"}),
    ] {
        let mut group = group_row();
        group["memberCount"] = count(bad);
        assert!(serde_json::from_value::<VizRow>(group).is_err());
    }
    Ok(())
}

#[test]
fn a_member_count_is_valid_only_on_a_group_row_and_only_in_items()
-> Result<(), Box<dyn std::error::Error>> {
    let exact = count(json!({"state":"exact","value":"2"}));
    // An item row has no members, so the field is refused on the wire, by the local
    // check, and inside a page.
    let mut item = item_row();
    item["memberCount"] = exact.clone();
    assert!(
        serde_json::from_value::<VizRow>(item.clone()).is_err(),
        "{item}"
    );
    let mut built: VizRow = serde_json::from_value(item_row())?;
    built.validate()?;
    built.member_count = serde_json::from_value(exact.clone())?;
    assert!(built.validate().is_err());
    let mut group = group_row();
    group["memberCount"] = exact.clone();
    let page = json!({
        "rows": [group, item],
        "totalCount": 2,
        "resultRevision": ID,
        "observedAt": null,
        "completeness": "complete"
    });
    assert!(serde_json::from_value::<VizRows>(page).is_err());

    // A row with neither reference is invalid, and the count does not repair it.
    let mut neither = item_row();
    neither["itemRef"] = Value::Null;
    neither["memberCount"] = exact;
    assert!(serde_json::from_value::<VizRow>(neither).is_err());

    // Only a count of items is a member count; null and other types are refused.
    let mut bytes = group_row();
    bytes["memberCount"] = json!({"unit":"bytes","state":"exact","value":"2"});
    assert!(serde_json::from_value::<VizRow>(bytes).is_err());
    for bad in [Value::Null, json!(2), json!("2")] {
        let mut group = group_row();
        group["memberCount"] = bad;
        assert!(serde_json::from_value::<VizRow>(group).is_err());
    }
    Ok(())
}

#[test]
fn a_request_without_a_folder_reference_means_the_root_table_as_before()
-> Result<(), Box<dyn std::error::Error>> {
    let old: VizRowsRequest = serde_json::from_value(request())?;
    assert_eq!(old.parent_shape_ref, None);
    assert!(old.validate().is_ok());
    assert!(serde_json::to_value(&old)?.get("parentShapeRef").is_none());

    let mut folder = request();
    folder["parentShapeRef"] = json!("s2-1");
    let decoded: VizRowsRequest = serde_json::from_value(folder.clone())?;
    assert_eq!(decoded.parent_shape_ref.as_deref(), Some("s2-1"));
    assert!(decoded.validate().is_ok());
    assert_eq!(serde_json::to_value(decoded)?, folder);
    Ok(())
}

#[test]
fn a_folder_reference_must_not_be_empty_or_null_and_unknown_fields_stay_refused()
-> Result<(), Box<dyn std::error::Error>> {
    for bad in [json!(""), Value::Null, json!(1)] {
        let mut value = request();
        value["parentShapeRef"] = bad;
        assert!(
            serde_json::from_value::<VizRowsRequest>(value.clone()).is_err(),
            "{value}"
        );
    }
    let mut built: VizRowsRequest = serde_json::from_value(request())?;
    built.parent_shape_ref = Some(String::new());
    assert!(built.validate().is_err());
    // The page limits are the ones of the root table.
    built.parent_shape_ref = Some("s2-1".into());
    for (count, accepted) in [(1, true), (1000, true), (0, false), (1001, false)] {
        built.count = count;
        assert_eq!(built.validate().is_ok(), accepted, "count {count}");
    }
    // A request is checked field by field, so a field this contract does not name is
    // refused as before.
    let mut value = request();
    value["parentShapeRef"] = json!("s2-1");
    value["shapeRef"] = json!("s2-1");
    assert!(serde_json::from_value::<VizRowsRequest>(value).is_err());
    Ok(())
}
