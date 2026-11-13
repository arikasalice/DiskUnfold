use super::*;
use du_analysis_index::visualization::model_links::ModelLinkError;

#[test]
fn final_validation_rejects_duplicate_missing_parent_and_cancellation() -> Result<(), String> {
    let (version, parent, keys, dispatcher) = fixture()?;
    let mut model = projection::project(
        source(&version, &parent, SizeBasis::Logical, &keys),
        &dispatcher,
        options(10),
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let root = &model.shapes[0];
    let row = &model.rows[root.row_index];
    let mut meta = root.meta.clone();
    meta.visible_depth = 2;
    meta.parent_shape_ref = Some(root.meta.shape_ref.clone());
    model.descendants.push(projection::DescendantShape {
        item: row.item.clone(),
        size: row.size.clone(),
        meta,
        geometry: root.geometry.clone(),
    });
    assert_eq!(
        projection::finish_child_partitions(&model, &mut || false),
        Err(projection::Error::Links(ModelLinkError::DuplicateShapeRef))
    );
    model.descendants[0].meta.shape_ref = "child".into();
    model.descendants[0].meta.parent_shape_ref = Some("missing".into());
    assert_eq!(
        projection::finish_child_partitions(&model, &mut || false),
        Err(projection::Error::Links(ModelLinkError::MissingParent))
    );
    model.descendants[0].meta.parent_shape_ref = Some(model.shapes[0].meta.shape_ref.clone());
    assert_eq!(
        projection::finish_child_partitions(&model, &mut || false),
        Ok(())
    );
    assert_eq!(
        projection::finish_child_partitions(&model, &mut || true),
        Err(projection::Error::Cancelled)
    );
    let mut checks = 0;
    assert_eq!(
        projection::finish_child_partitions(&model, &mut || {
            checks += 1;
            checks > model.shapes.len() + model.descendants.len() + 1
        }),
        Err(projection::Error::Cancelled)
    );
    Ok(())
}
