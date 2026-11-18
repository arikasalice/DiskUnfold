//! Pins the optional fields of the `space.total` success body.
use du_core_contracts::{
    Id, analysis_ipc::SpaceTotal, browse_page_ipc::QueryStrategy, result::ResultCompleteness,
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
    }
}

#[test]
fn a_response_without_the_new_fields_still_decodes() -> Result<(), serde_json::Error> {
    let decoded: SpaceTotal = serde_json::from_value(base())?;
    assert_eq!(decoded.size_rule, None);
    assert_eq!(decoded.cancelled_unread_folders, None);
    Ok(())
}

#[test]
fn omitted_fields_are_not_written() -> Result<(), serde_json::Error> {
    let written = serde_json::to_value(total(None, None))?;
    assert_eq!(written, base());
    assert_eq!(written.get("sizeRule"), None);
    assert_eq!(written.get("cancelledUnreadFolders"), None);
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
