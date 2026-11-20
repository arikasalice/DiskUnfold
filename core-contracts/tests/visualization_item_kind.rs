//! The optional item kind of visualization rows (#183).
use du_core_contracts::result::ResultItemKind;
use du_core_contracts::visualization::{item_kind::VizItemKind, rows::VizRow};
use serde_json::{Value, json};
const ID: &str = "018f0000-0000-7000-8000-000000000001";
fn row() -> Value {
    json!({"itemRef":{"resultVersion":ID,"itemNo":1},"groupRef":null,"displayName":"a","size":{"basis":"logical","evidence":"measured","state":"exact","bytes":"20"},"ratio":null,"denominator":{"basis":"scope","value":{"unit":"bytes","state":"exact","value":"80"}},"legendClassKey":"class","displayFlags":[]})
}

#[test]
fn an_item_row_round_trips_with_and_without_its_kind() -> Result<(), Box<dyn std::error::Error>> {
    let old: VizRow = serde_json::from_value(row())?;
    assert_eq!(old.item_kind, None);
    assert!(serde_json::to_value(&old)?.get("itemKind").is_none());
    for (text, kind) in [
        ("file", VizItemKind::File),
        ("folder", VizItemKind::Folder),
        ("other", VizItemKind::Other),
    ] {
        let mut value = row();
        value["itemKind"] = json!(text);
        let read: VizRow = serde_json::from_value(value.clone())?;
        assert_eq!(read.item_kind, Some(kind));
        assert_eq!(serde_json::to_value(&read)?, value);
    }
    Ok(())
}

#[test]
fn an_unknown_kind_reads_as_omitted_and_null_or_a_non_string_is_refused()
-> Result<(), Box<dyn std::error::Error>> {
    let mut future = row();
    future["itemKind"] = json!("device");
    let read: VizRow = serde_json::from_value(future)?;
    assert_eq!(read.item_kind, None);
    assert!(serde_json::to_value(&read)?.get("itemKind").is_none());
    for bad in [Value::Null, json!(1)] {
        let mut value = row();
        value["itemKind"] = bad;
        assert!(serde_json::from_value::<VizRow>(value).is_err());
    }
    Ok(())
}

#[test]
fn a_group_row_with_a_kind_is_refused() {
    let mut group = row();
    group["itemRef"] = Value::Null;
    group["groupRef"] = json!("g1");
    assert!(serde_json::from_value::<VizRow>(group.clone()).is_ok());
    group["itemKind"] = json!("folder");
    assert!(serde_json::from_value::<VizRow>(group).is_err());
}

#[test]
fn each_result_kind_maps_and_an_unknown_kind_has_none() {
    for (kind, expected) in [
        (ResultItemKind::Directory, Some(VizItemKind::Folder)),
        (ResultItemKind::File, Some(VizItemKind::File)),
        (ResultItemKind::UntraversedLink, Some(VizItemKind::Other)),
        (ResultItemKind::Unknown, None),
        (ResultItemKind::UnknownStructure, None),
    ] {
        assert_eq!(VizItemKind::from_result(kind), expected, "{kind:?}");
    }
}
