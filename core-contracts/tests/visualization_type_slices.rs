//! #298: a folder row's type slices on the wire (F06 README 54).
use du_core_contracts::visualization::{rows::VizRow, type_slices::MAX_TYPE_SLICES};
use serde_json::{Value, json};
const ID: &str = "018f0000-0000-7000-8000-000000000001";

fn folder_row() -> Value {
    json!({"itemRef":{"resultVersion":ID,"itemNo":1},"groupRef":null,"displayName":"a","size":{"basis":"logical","evidence":"measured","state":"exact","bytes":"20"},"ratio":"-0.25","denominator":{"basis":"scope","value":{"unit":"bytes","state":"exact","value":"80"}},"legendClassKey":"type.folder","displayFlags":[],"itemKind":"folder"})
}

fn slice(key: &str, state: &str, bytes: &str) -> Value {
    json!({"classKey": key, "itemCount": {"unit":"items","state":state,"value":"2"}, "total": {"unit":"bytes","state":state,"value":bytes}})
}

fn with(slices: Value) -> Value {
    let mut row = folder_row();
    row["typeSlices"] = slices;
    row
}

#[test]
fn a_folder_row_reads_and_writes_its_slices() -> Result<(), Box<dyn std::error::Error>> {
    for slices in [
        json!([
            slice("type.video", "exact", "12"),
            slice("type.document", "lower_bound", "8")
        ]),
        // Every file was counted and there is none.
        json!([]),
    ] {
        let parsed: VizRow = serde_json::from_value(with(slices.clone()))?;
        assert_eq!(serde_json::to_value(&parsed)?["typeSlices"], slices);
    }
    // Omitted means not applicable, and stays omitted.
    let parsed: VizRow = serde_json::from_value(folder_row())?;
    assert!(parsed.type_slices.is_none());
    assert!(serde_json::to_value(&parsed)?.get("typeSlices").is_none());
    Ok(())
}

#[test]
fn slices_are_refused_when_null_repeated_unknown_or_on_an_item_row_that_is_not_a_folder() {
    let unknown = json!({"classKey":"type.video","itemCount":{"unit":"items","state":"exact","value":"1"},"total":{"unit":"bytes","state":"unknown","reasonKey":"space.data_insufficient"}});
    let in_count = json!({"classKey":"type.video","itemCount":{"unit":"bytes","state":"exact","value":"1"},"total":{"unit":"bytes","state":"exact","value":"1"}});
    for bad in [
        Value::Null,
        json!([
            slice("type.video", "exact", "1"),
            slice("type.video", "exact", "2")
        ]),
        json!([slice("", "exact", "1")]),
        json!([unknown]),
        json!([in_count]),
    ] {
        assert!(
            serde_json::from_value::<VizRow>(with(bad.clone())).is_err(),
            "{bad}"
        );
    }
    let one = json!([slice("type.video", "exact", "1")]);
    let mut file = with(one.clone());
    file["itemKind"] = json!("file");
    let mut unknown_kind = with(one.clone());
    unknown_kind
        .as_object_mut()
        .map(|row| row.remove("itemKind"));
    let mut group = with(one);
    group["itemRef"] = Value::Null;
    group["groupRef"] = json!("g1");
    group.as_object_mut().map(|row| row.remove("itemKind"));
    // The Other group carries its members' slices (README 54.2); another group does not.
    let mut other_group = group.clone();
    other_group["labelKey"] = json!("ui.visualization.group.other");
    other_group["displayName"] = json!("ui.visualization.group.other");
    assert!(serde_json::from_value::<VizRow>(other_group).is_ok());
    for bad in [file, unknown_kind, group] {
        assert!(
            serde_json::from_value::<VizRow>(bad.clone()).is_err(),
            "{bad}"
        );
    }
}

#[test]
fn a_full_page_of_folders_with_every_type_stays_small() -> Result<(), Box<dyn std::error::Error>> {
    // One slice per legend class (nine today), every value at its widest.
    let widest = u64::MAX.to_string();
    let nine: Vec<Value> = (0..9)
        .map(|n| slice(&format!("type.class{n}"), "lower_bound", &widest))
        .collect();
    let row = with(json!(nine));
    serde_json::from_value::<VizRow>(row.clone())?;
    let page = json!({"rows": vec![row; 1000]});
    let bytes = serde_json::to_vec(&page)?.len();
    // About 1.6 KiB a row: a full page of 1000 rows stays under 2 MiB.
    assert!(bytes < 2 * 1024 * 1024, "{bytes}");
    // A list longer than any legend is refused, which bounds every row.
    let too_many: Vec<Value> = (0..=MAX_TYPE_SLICES)
        .map(|n| slice(&format!("k{n}"), "exact", "1"))
        .collect();
    assert!(serde_json::from_value::<VizRow>(with(json!(too_many))).is_err());
    Ok(())
}
