use du_core_contracts::{
    Id, ItemRef,
    visualization::model::{ModelLinkError, ShapeMeta, VizRelationship, validate_model_links},
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
fn preserves_open_relationship_kinds_and_geometry_with_valid_endpoints()
-> Result<(), Box<dyn std::error::Error>> {
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
    assert_eq!(
        validate_model_links(&revision, &shapes, std::slice::from_ref(&relation)),
        Ok(())
    );
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

#[test]
fn rejects_broken_hierarchy_and_references_before_model_publication()
-> Result<(), Box<dyn std::error::Error>> {
    let revision = Id::parse("018f0000-0000-7000-8000-000000000001")?;
    let other = Id::parse("018f0000-0000-7000-8000-000000000002")?;
    let mut wrong_branch = shape("leaf", None, 0);
    wrong_branch.root_branch_ref = Some(ItemRef {
        result_version: other,
        item_no: 5,
    });
    for (shapes, error) in [
        (vec![shape("", None, 0)], ModelLinkError::InvalidShapeRef),
        (
            vec![shape("same", None, 0), shape("same", None, 1)],
            ModelLinkError::DuplicateShapeRef,
        ),
        (
            vec![shape("leaf", Some("unknown"), 1)],
            ModelLinkError::MissingParent,
        ),
        (
            vec![shape("self", Some("self"), 0)],
            ModelLinkError::InvalidParentDepth,
        ),
        (
            vec![shape("a", Some("b"), 1), shape("b", Some("a"), 2)],
            ModelLinkError::InvalidParentDepth,
        ),
        (vec![wrong_branch], ModelLinkError::BranchVersionMismatch),
    ] {
        assert_eq!(validate_model_links(&revision, &shapes, &[]), Err(error));
    }
    let unrelated = VizRelationship {
        kind: "space_attribution".into(),
        from_shape_ref: "root".into(),
        to_shape_ref: "other-model".into(),
        geometry: None,
    };
    assert_eq!(
        validate_model_links(&revision, &[shape("root", None, 0)], &[unrelated]),
        Err(ModelLinkError::MissingEndpoint)
    );
    Ok(())
}
