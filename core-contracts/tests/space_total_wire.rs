//! Pins the optional fields of the `space.total` success body.
use du_core_contracts::{
    Id, Timestamp,
    analysis_ipc::{SpaceObservationRange, SpaceTotal},
    browse_page_ipc::QueryStrategy,
    result::ResultCompleteness,
};
use serde_json::{Value, json};

fn base() -> Value {
    json!({
        "resultVersion": "018f0000-0000-7000-8000-000000000001",
        "requestSeq": 7,
        "sizes": [],
        "completeness": "complete",
        "missingItems": null,
        "inaccessibleRanges": null,
        "untraversedLinks": null
    })
}

fn total(size_rule: Option<QueryStrategy>, cancelled: Option<u32>) -> SpaceTotal {
    let result_version = Id::parse("018f0000-0000-7000-8000-000000000001")
        .unwrap_or_else(|_| unreachable!("a valid identifier"));
    SpaceTotal {
        result_version,
        request_seq: 7,
        sizes: Vec::new(),
        completeness: ResultCompleteness::Complete,
        missing_items: None,
        inaccessible_ranges: None,
        untraversed_links: None,
        size_rule,
        cancelled_unread_folders: cancelled,
        observation_range: None,
    }
}

#[test]
fn a_response_without_the_new_fields_still_decodes() -> Result<(), serde_json::Error> {
    let decoded: SpaceTotal = serde_json::from_value(base())?;
    assert_eq!(decoded.size_rule, None);
    assert_eq!(decoded.cancelled_unread_folders, None);
    assert_eq!(decoded.observation_range, None);
    Ok(())
}

#[test]
fn omitted_fields_are_not_written() -> Result<(), serde_json::Error> {
    let written = serde_json::to_value(total(None, None))?;
    assert_eq!(written, base());
    assert_eq!(written.get("sizeRule"), None);
    assert_eq!(written.get("cancelledUnreadFolders"), None);
    assert_eq!(written.get("observationRange"), None);
    Ok(())
}

#[test]
fn zero_and_omission_of_the_cancelled_count_are_distinct() -> Result<(), serde_json::Error> {
    let zero = serde_json::to_value(total(None, Some(0)))?;
    assert_eq!(zero.get("cancelledUnreadFolders"), Some(&json!(0)));
    let decoded: SpaceTotal = serde_json::from_value(zero)?;
    assert_eq!(decoded.cancelled_unread_folders, Some(0));
    let omitted = serde_json::to_value(total(None, None))?;
    assert_eq!(omitted.get("cancelledUnreadFolders"), None);
    let decoded: SpaceTotal = serde_json::from_value(omitted)?;
    assert_eq!(decoded.cancelled_unread_folders, None);
    Ok(())
}

#[test]
fn the_new_fields_use_their_published_names_and_shape() -> Result<(), serde_json::Error> {
    let rule = QueryStrategy {
        id: "size.agg.subtree-unique".into(),
        revision: 1,
    };
    let written = serde_json::to_value(total(Some(rule), Some(3)))?;
    let mut expected = base();
    expected["sizeRule"] = json!({"id": "size.agg.subtree-unique", "revision": 1});
    expected["cancelledUnreadFolders"] = json!(3);
    assert_eq!(written, expected);
    let decoded: SpaceTotal = serde_json::from_value(expected)?;
    assert_eq!(decoded.cancelled_unread_folders, Some(3));
    assert_eq!(
        decoded.size_rule.map(|rule| (rule.id, rule.revision)),
        Some(("size.agg.subtree-unique".to_owned(), 1))
    );
    Ok(())
}

#[test]
fn unknown_fields_of_a_newer_producer_are_accepted() -> Result<(), serde_json::Error> {
    let mut future = base();
    future["someFutureField"] = json!({"x": 1});
    let decoded: SpaceTotal = serde_json::from_value(future)?;
    assert_eq!(decoded.request_seq, 7);
    Ok(())
}

fn at(text: &str) -> Result<Timestamp, String> {
    Timestamp::parse(text).map_err(|error| format!("{error:?}"))
}

fn with_range(range: Value) -> Value {
    let mut body = base();
    body["observationRange"] = range;
    body
}

#[test]
fn the_observation_range_round_trips_with_all_three_keys() -> Result<(), String> {
    let range = SpaceObservationRange::new(
        Some(at("2026-11-18T01:00:00.000Z")?),
        Some(at("2026-11-18T02:30:00.000Z")?),
        false,
    )?;
    let mut response = total(None, None);
    response.observation_range = Some(range.clone());
    let written = serde_json::to_value(&response).map_err(|e| e.to_string())?;
    let expected = with_range(json!({
        "earliest": "2026-11-18T01:00:00.000Z",
        "latest": "2026-11-18T02:30:00.000Z",
        "complete": false
    }));
    assert_eq!(written, expected);
    let decoded: SpaceTotal = serde_json::from_value(expected).map_err(|e| e.to_string())?;
    assert_eq!(decoded.observation_range, Some(range));
    Ok(())
}

#[test]
fn a_confirmed_empty_set_keeps_null_bounds_and_is_distinct_from_absence() -> Result<(), String> {
    let empty = SpaceObservationRange::new(None, None, true)?;
    let mut response = total(None, None);
    response.observation_range = Some(empty.clone());
    let written = serde_json::to_value(&response).map_err(|e| e.to_string())?;
    assert_eq!(
        written["observationRange"],
        json!({"earliest": null, "latest": null, "complete": true})
    );
    let decoded: SpaceTotal = serde_json::from_value(written).map_err(|e| e.to_string())?;
    assert_eq!(decoded.observation_range, Some(empty));
    let null: SpaceTotal =
        serde_json::from_value(with_range(Value::Null)).map_err(|e| e.to_string())?;
    assert_eq!(null.observation_range, None);
    Ok(())
}

#[test]
fn one_missing_bound_and_reversed_bounds_are_rejected() -> Result<(), String> {
    let early = at("2026-11-18T01:00:00.000Z")?;
    let late = at("2026-11-18T02:00:00.000Z")?;
    assert!(SpaceObservationRange::new(Some(early.clone()), None, true).is_err());
    assert!(SpaceObservationRange::new(None, Some(late.clone()), true).is_err());
    assert!(SpaceObservationRange::new(Some(late.clone()), Some(early.clone()), true).is_err());
    let same = SpaceObservationRange::new(Some(early.clone()), Some(early.clone()), true)?;
    assert_eq!(same.earliest(), same.latest());
    for range in [
        json!({"earliest": "2026-11-18T01:00:00.000Z", "latest": null, "complete": true}),
        json!({"earliest": null, "latest": "2026-11-18T01:00:00.000Z", "complete": true}),
        json!({
            "earliest": "2026-11-18T02:00:00.000Z",
            "latest": "2026-11-18T01:00:00.000Z",
            "complete": true
        }),
        json!({"earliest": "2026-11-18 01:00:00", "latest": "2026-11-18 01:00:00", "complete": true}),
    ] {
        assert!(
            serde_json::from_value::<SpaceTotal>(with_range(range.clone())).is_err(),
            "{range}"
        );
    }
    Ok(())
}

#[test]
fn unknown_fields_inside_the_observation_range_are_accepted() -> Result<(), String> {
    let decoded: SpaceTotal = serde_json::from_value(with_range(json!({
        "earliest": null,
        "latest": null,
        "complete": false,
        "someFutureField": 1
    })))
    .map_err(|e| e.to_string())?;
    assert_eq!(
        decoded.observation_range,
        Some(SpaceObservationRange::new(None, None, false)?)
    );
    Ok(())
}
