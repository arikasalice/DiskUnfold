use du_core_contracts::{
    Id, ItemRef,
    visualization::model::{ShapeMeta, VizRelationship},
};
use serde_json::{from_value, json, to_value};

fn shape(key: &str, parent: Option<&str>, depth: u32) -> ShapeMeta {
    ShapeMeta {
        shape_ref: key.into(),
        parent_shape_ref: parent.map(str::to_owned),
        visible_depth: depth,
        root_branch_ref: None,
    }
}

#[test]
fn preserves_open_relationship_kinds_and_geometry() -> Result<(), Box<dyn std::error::Error>> {
    let revision = Id::parse("018f0000-0000-7000-8000-000000000001")?;
    let mut child = shape("leaf", Some("root"), 3);
    child.root_branch_ref = Some(ItemRef {
        result_version: revision.clone(),
        item_no: 5,
    });
    let shapes = [child, shape("root", None, 0)];
    let serialized = json!({
        "kind": "future_flow",
        "fromShapeRef": "root",
        "toShapeRef": "leaf",
        "geometry": {"primitive": "future_arc", "points": [{"x": 7, "y": 9}]},
    });
    let relation: VizRelationship = from_value(serialized.clone())?;
    assert_eq!(to_value(relation)?, serialized);
    assert_eq!(
        to_value(&shapes[1])?,
        json!({
            "shapeRef": "root",
            "parentShapeRef": null,
            "visibleDepth": 0,
            "rootBranchRef": null,
        })
    );
    Ok(())
}
