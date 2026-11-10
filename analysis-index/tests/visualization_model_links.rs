use du_analysis_index::visualization::model_links::{ModelLinkError, validate_model_links};
use du_core_contracts::{
    Id, ItemRef,
    visualization::model::{ShapeMeta, VizRelationship},
};

fn shape(key: &str, parent: Option<&str>, depth: u32) -> ShapeMeta {
    ShapeMeta {
        shape_ref: key.into(),
        parent_shape_ref: parent.map(str::to_owned),
        visible_depth: depth,
        root_branch_ref: None,
    }
}

#[test]
fn accepts_an_omitted_ancestor_and_an_open_relationship_kind()
-> Result<(), Box<dyn std::error::Error>> {
    let revision = Id::parse("018f0000-0000-7000-8000-000000000001")?;
    let mut child = shape("leaf", Some("root"), 3);
    child.root_branch_ref = Some(ItemRef {
        result_version: revision.clone(),
        item_no: 5,
    });
    let relation = VizRelationship {
        kind: "future_flow".into(),
        from_shape_ref: "root".into(),
        to_shape_ref: "leaf".into(),
        geometry: None,
    };
    assert_eq!(
        validate_model_links(&revision, &[child, shape("root", None, 0)], &[relation]),
        Ok(())
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
