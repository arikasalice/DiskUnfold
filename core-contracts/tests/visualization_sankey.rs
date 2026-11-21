//! #296: Sankey columns and row links on the wire (F06 README 12.1).
use du_core_contracts::visualization::{
    rows::VizRow,
    sankey::{MAX_ROW_LINKS, VizColumn, VizLink},
};
use serde_json::{Value, json};
const ID: &str = "018f0000-0000-7000-8000-000000000001";

fn node_row() -> Value {
    json!({"itemRef":{"resultVersion":ID,"itemNo":1},"groupRef":null,"displayName":"a","size":{"basis":"logical","evidence":"measured","state":"exact","bytes":"20"},"ratio":null,"denominator":{"basis":"scope","value":{"unit":"bytes","state":"exact","value":"80"}},"legendClassKey":"type.folder","displayFlags":[],"itemKind":"folder"})
}

fn link(to: &str, state: &str, value: &str) -> Value {
    json!({"toRowRef": to, "quantity": {"unit": "bytes", "state": state, "value": value}})
}

#[test]
fn a_node_row_reads_and_writes_its_links() -> Result<(), Box<dyn std::error::Error>> {
    let links = json!([
        link("viz.sankey.type.type.video", "exact", "12"),
        link("viz.sankey.type.type.document", "exact", "0")
    ]);
    let mut row = node_row();
    row["links"] = links.clone();
    let parsed: VizRow = serde_json::from_value(row)?;
    assert_eq!(serde_json::to_value(&parsed)?["links"], links);
    let plain: VizRow = serde_json::from_value(node_row())?;
    assert!(serde_json::to_value(&plain)?.get("links").is_none());
    Ok(())
}

#[test]
fn links_are_refused_when_null_repeated_inexact_or_too_many() {
    let too_many: Vec<Value> = (0..=MAX_ROW_LINKS)
        .map(|n| link(&format!("r{n}"), "exact", "1"))
        .collect();
    for bad in [
        Value::Null,
        json!([link("r", "exact", "1"), link("r", "exact", "2")]),
        json!([link("", "exact", "1")]),
        json!([link("r", "lower_bound", "1")]),
        json!([{"toRowRef":"r","quantity":{"unit":"items","state":"exact","value":"1"}}]),
        json!(too_many),
    ] {
        let mut row = node_row();
        row["links"] = bad.clone();
        assert!(serde_json::from_value::<VizRow>(row).is_err(), "{bad}");
    }
}

#[test]
fn columns_link_one_step_right_and_an_unknown_column_is_read()
-> Result<(), Box<dyn std::error::Error>> {
    assert!(VizColumn::Scope.links_to(VizColumn::Folder));
    assert!(VizColumn::Folder.links_to(VizColumn::Type));
    assert!(!VizColumn::Scope.links_to(VizColumn::Type));
    assert!(!VizColumn::Type.links_to(VizColumn::Folder));
    let later: VizColumn = serde_json::from_value(json!("later"))?;
    assert_eq!(later, VizColumn::Unknown);
    assert!(VizLink::exact_bytes(&serde_json::from_value(
        json!({"unit":"bytes","state":"exact","value":"0"})
    )?));
    Ok(())
}
