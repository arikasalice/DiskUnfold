use du_core_contracts::visualization::rows::{
    VizGroupRows, VizGroupRowsRequest, VizRatio, VizRow, VizRows, VizRowsRequest,
};
use serde_json::{Value, json};
const ID: &str = "018f0000-0000-7000-8000-000000000001";
const OTHER: &str = "018f0000-0000-7000-8000-000000000002";
fn row() -> Value {
    json!({"itemRef":{"resultVersion":ID,"itemNo":1},"groupRef":null,"displayName":"a","size":{"basis":"logical","evidence":"measured","state":"exact","bytes":"20"},"ratio":"-0.25","denominator":{"basis":"scope","value":{"unit":"bytes","state":"exact","value":"80"}},"legendClassKey":"future.class","displayFlags":["future.flag"]})
}
/// The same row as a group row, which is the only kind that may carry a label key.
fn group_row() -> Value {
    let mut group = row();
    group["itemRef"] = Value::Null;
    group["groupRef"] = json!("g1");
    group
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
fn label_key_round_trips_and_an_old_row_without_it_is_still_read()
-> Result<(), Box<dyn std::error::Error>> {
    // A row from before the field existed has no labelKey and writes none back.
    let old: VizRow = serde_json::from_value(row())?;
    assert_eq!(old.label_key, None);
    assert!(serde_json::to_value(&old)?.get("labelKey").is_none());
    let mut group = group_row();
    group["displayName"] = json!("ui.visualization.group.other");
    group["labelKey"] = json!("ui.visualization.group.other");
    let decoded: VizRow = serde_json::from_value(group.clone())?;
    assert_eq!(
        decoded.label_key.as_deref(),
        Some("ui.visualization.group.other")
    );
    assert_eq!(serde_json::to_value(decoded)?, group);
    // A page carrying such a row keeps it through the page check and a round trip.
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
fn label_key_rejects_empty_and_null_and_unknown_fields_stay_accepted() {
    for bad in [json!(""), Value::Null, json!(1), json!(["k"])] {
        let mut v = group_row();
        v["labelKey"] = bad;
        assert!(serde_json::from_value::<VizRow>(v.clone()).is_err(), "{v}");
    }
    let mut v = group_row();
    v["labelKey"] = json!("ui.visualization.group.other");
    v["futureField"] = json!({"any": true});
    assert!(serde_json::from_value::<VizRow>(v).is_ok());
    let mut empty = serde_json::from_value::<VizRow>(group_row()).ok();
    if let Some(row) = &mut empty {
        row.label_key = Some(String::new());
        assert!(row.validate().is_err());
        row.label_key = Some("k".into());
        assert!(row.validate().is_ok());
    }
}

#[test]
fn label_key_is_valid_only_on_a_group_row() -> Result<(), Box<dyn std::error::Error>> {
    // A group row, which has a group reference and no item reference, may carry it.
    let mut group = group_row();
    group["labelKey"] = json!("ui.visualization.group.other");
    let group: VizRow = serde_json::from_value(group)?;
    group.validate()?;

    // An item row would show the message text in place of a real file name, so it is
    // refused on the wire and by the local check.
    let mut item = row();
    item["labelKey"] = json!("ui.visualization.group.other");
    assert!(
        serde_json::from_value::<VizRow>(item.clone()).is_err(),
        "{item}"
    );
    let mut built: VizRow = serde_json::from_value(row())?;
    built.validate()?;
    built.label_key = Some("ui.visualization.group.other".into());
    assert!(built.validate().is_err());
    let Some(revision) = built
        .item_ref
        .as_ref()
        .map(|item| item.result_version.clone())
    else {
        return Err("the item row has no item reference".into());
    };
    assert!(built.validate_revision(&revision).is_err());

    // The check also guards a page: one item row with a key rejects the whole page.
    let page = json!({
        "rows": [group_row(), item],
        "totalCount": 2,
        "resultRevision": ID,
        "observedAt": null,
        "completeness": "complete"
    });
    assert!(serde_json::from_value::<VizRows>(page).is_err());
    Ok(())
}

#[test]
fn label_key_is_refused_on_a_row_without_any_reference() -> Result<(), Box<dyn std::error::Error>> {
    // A row with neither an item nor a group reference is invalid by itself. A label
    // key does not make it a group row: it is refused on the wire, by the local check
    // and inside a page alike.
    let mut neither = row();
    neither["itemRef"] = Value::Null;
    neither["labelKey"] = json!("ui.visualization.group.other");
    assert!(
        serde_json::from_value::<VizRow>(neither.clone()).is_err(),
        "{neither}"
    );
    let mut built: VizRow = serde_json::from_value(group_row())?;
    built.validate()?;
    built.item_ref = None;
    built.group_ref = None;
    built.label_key = Some("ui.visualization.group.other".into());
    assert!(built.validate().is_err());
    let page = json!({
        "rows": [neither],
        "totalCount": 1,
        "resultRevision": ID,
        "observedAt": null,
        "completeness": "complete"
    });
    assert!(serde_json::from_value::<VizRows>(page).is_err());
    Ok(())
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
    let mut extended = request.clone();
    extended["futureField"] = json!(true);
    assert!(serde_json::from_value::<VizRowsRequest>(extended).is_err());
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
    use du_core_contracts::visualization::rows::{
        VizDenominator, VizDenominatorBasis, VizSizeRange,
    };
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
        VizSizeRange::decl(&config),
        VizRows::decl(&config),
        VizGroupRows::decl(&config),
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
fn group_rows_response_round_trips_required_evidence_and_rejects_invalid_pages()
-> Result<(), Box<dyn std::error::Error>> {
    let known_range = json!({"kind":"known", "min":{"basis":"logical","evidence":"measured","state":"exact","bytes":"20"}, "max":{"basis":"logical","evidence":"measured","state":"exact","bytes":"40"}});
    let make_page = |range: Value| {
        json!({
            "rows":[row()],
            "totalCount":1,
            "groupTotal":{"basis":"logical","evidence":"measured","state":"exact","bytes":"60"},
            "sizeRange":range,
            "resultRevision":ID,
            "observedAt":null,
            "completeness":"complete"
        })
    };

    // The page and its size range accept fields added by a later revision.
    let mut extended = make_page(known_range.clone());
    extended["futureField"] = json!(true);
    extended["sizeRange"]["futureField"] = json!(true);
    let parsed: VizGroupRows = serde_json::from_value(extended)?;
    assert_eq!(
        serde_json::to_value(parsed)?,
        make_page(known_range.clone())
    );

    for range in [known_range, json!({"kind":"unknown"})] {
        let value = make_page(range);
        let parsed: VizGroupRows = serde_json::from_value(value.clone())?;
        parsed.validate()?;
        assert_eq!(serde_json::to_value(parsed)?, value);
    }

    let valid = make_page(json!({"kind":"unknown"}));
    let mut wrong_revision = valid.clone();
    wrong_revision["resultRevision"] = json!(OTHER);
    assert!(serde_json::from_value::<VizGroupRows>(wrong_revision).is_err());
    let mut too_many_rows = valid.clone();
    too_many_rows["totalCount"] = json!(0);
    assert!(serde_json::from_value::<VizGroupRows>(too_many_rows).is_err());
    let mut unsafe_count = valid.clone();
    unsafe_count["totalCount"] = json!(9007199254740992_u64);
    assert!(serde_json::from_value::<VizGroupRows>(unsafe_count).is_err());

    for malformed_range in [
        json!({"kind":"known", "min":{"basis":"logical","evidence":"measured","state":"exact","bytes":"20"}}),
        json!({"kind":"unknown", "min":{"basis":"logical","evidence":"measured","state":"exact","bytes":"20"}}),
        json!({"kind":"unknown", "min":null}),
        json!({"kind":"future"}),
        json!({"min":{"basis":"logical","evidence":"measured","state":"exact","bytes":"20"},"max":{"basis":"logical","evidence":"measured","state":"exact","bytes":"40"}}),
        json!({"kind":"known", "min":{"basis":"logical","evidence":"measured","state":"exact","bytes":"20"}, "max":{"basis":"allocated","evidence":"measured","state":"exact","bytes":"40"}}),
        json!({"kind":"known", "min":{"basis":"logical","evidence":"measured","state":"exact","bytes":"41"}, "max":{"basis":"logical","evidence":"measured","state":"exact","bytes":"40"}}),
        json!({"kind":"known", "min":{"basis":"logical","evidence":"unknown","state":"unknown"}, "max":{"basis":"logical","evidence":"measured","state":"exact","bytes":"40"}}),
        json!({"kind":"known", "min":{"basis":"logical","evidence":"measured","state":"lower_bound","bytes":"20"}, "max":{"basis":"logical","evidence":"measured","state":"exact","bytes":"40"}}),
        json!({"kind":"known", "min":{"basis":"allocated","evidence":"measured","state":"exact","bytes":"20"}, "max":{"basis":"allocated","evidence":"measured","state":"exact","bytes":"40"}}),
        json!({"kind":"known", "min":{"basis":"logical","evidence":"unknown","state":"exact","bytes":"20"}, "max":{"basis":"logical","evidence":"measured","state":"exact","bytes":"40"}}),
    ] {
        assert!(
            serde_json::from_value::<VizGroupRows>(make_page(malformed_range.clone())).is_err(),
            "{malformed_range}"
        );
    }

    for field in [
        "groupTotal",
        "sizeRange",
        "resultRevision",
        "observedAt",
        "completeness",
    ] {
        let mut missing = valid.clone();
        missing.as_object_mut().map(|o| o.remove(field));
        assert!(
            serde_json::from_value::<VizGroupRows>(missing).is_err(),
            "{field}"
        );
    }
    Ok(())
}

#[test]
fn a_list_condition_rides_only_on_a_group_row_and_is_never_null()
-> Result<(), Box<dyn std::error::Error>> {
    let condition = json!({"kind":"all","children":[
        {"kind":"predicate","predicate":{"kind":"type","value":"file"}},
        {"kind":"predicate","predicate":{"kind":"size","basis":"logical","op":"at_least","bytes":"1000"}},
        {"kind":"predicate","predicate":{"kind":"size","basis":"logical","op":"less_than","bytes":"1000000"}}
    ]});
    let mut group = group_row();
    group["condition"] = condition.clone();
    let parsed: VizRow = serde_json::from_value(group.clone())?;
    assert_eq!(serde_json::to_value(&parsed)?, group);
    // Without it a row reads as before and writes no field.
    let plain: VizRow = serde_json::from_value(group_row())?;
    assert!(plain.condition.is_none());
    assert!(serde_json::to_value(&plain)?.get("condition").is_none());
    let mut item = row();
    item["condition"] = condition;
    assert!(serde_json::from_value::<VizRow>(item).is_err());
    let mut null = group_row();
    null["condition"] = Value::Null;
    assert!(serde_json::from_value::<VizRow>(null).is_err());
    Ok(())
}

#[test]
fn a_folder_row_says_how_it_could_not_be_read() -> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::visualization::rows::VizUnread;
    let with = |unread: Value| {
        let mut value = row();
        value["unread"] = unread;
        value
    };
    for (wire, expected) in [
        (json!({"kind":"itself"}), VizUnread::Itself),
        (
            json!({"kind":"inside","count":"3"}),
            VizUnread::Inside {
                count: Some("3".into()),
            },
        ),
        (json!({"kind":"inside"}), VizUnread::Inside { count: None }),
        // An unknown kind is read, so a newer sender does not break an older receiver.
        (json!({"kind":"later"}), VizUnread::Unknown),
    ] {
        let parsed: VizRow = serde_json::from_value(with(wire.clone()))?;
        assert_eq!(parsed.unread, Some(expected), "{wire}");
    }
    assert_eq!(VizUnread::inside(Some(0)), None);
    for bad in [
        Value::Null,
        json!({"kind":"inside","count":"0"}),
        json!({"kind":"inside","count":"03"}),
        json!({"kind":"inside","count":"-3"}),
        json!({"kind":"inside","count":null}),
    ] {
        assert!(
            serde_json::from_value::<VizRow>(with(bad.clone())).is_err(),
            "{bad}"
        );
    }
    let mut group = group_row();
    group["unread"] = json!({"kind":"itself"});
    assert!(serde_json::from_value::<VizRow>(group).is_err());
    Ok(())
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
