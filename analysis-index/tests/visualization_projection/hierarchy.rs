use super::*;
use du_analysis_index::visualization::model_links::ModelLinkError;

#[test]
fn final_validation_rejects_duplicate_missing_parent_and_cancellation() -> Result<(), String> {
    let (version, parent, dispatcher) = fixture()?;
    let mut model = projection::project(
        source(&version, &parent, SizeBasis::Logical),
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

#[test]
fn equal_size_children_of_a_branch_are_added_in_name_order() -> Result<(), String> {
    let id = |n| Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"));
    let sink = IngestSink::default();
    let mut builder = sink.open(id(21)?).map_err(|e| format!("{e:?}"))?;
    builder
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    builder
        .add_directory(vec![b"folder".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    for name in [b"q", b"p"] {
        builder
            .add_file(Observation {
                path: vec![b"folder".to_vec(), name.to_vec()],
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact {
                        bytes: 10_u64.into(),
                    },
                }),
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = builder
        .seal(id(22)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let parent = ItemRef {
        result_version: version.version_id().clone(),
        item_no: 0,
    };
    let (_, _, dispatcher) = fixture()?;
    let mut model = projection::project(
        source(&version, &parent, SizeBasis::Logical),
        &dispatcher,
        options(10),
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    // No comparison key reaches the projection; the tied pair is laid out by name.
    projection::add_child_partition(&mut model, &version, 0, &dispatcher, 10, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    let names: Vec<_> = model
        .descendants
        .iter()
        .map(|shape| version.items()[shape.item.item_no as usize].name.clone())
        .collect();
    assert_eq!(names, [b"p".to_vec(), b"q".to_vec()]);
    Ok(())
}
