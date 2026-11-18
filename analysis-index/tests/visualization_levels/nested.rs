use super::*;
use du_analysis_index::visualization::projection::{add_child_partition, finish_child_partitions};

fn tree() -> Arc<ResultVersion> {
    let mut entries = Vec::new();
    for folder in ["a", "b"] {
        for i in 0..100 {
            entries.push((format!("{folder}/f{i:03}"), 10));
        }
    }
    build(&entries, ResultCompleteness::Complete)
}

fn expand(
    model: &mut Model,
    version: &ResultVersion,
    shape: usize,
    max_shapes: usize,
) -> Result<(), projection::Error> {
    add_child_partition(
        model,
        version,
        shape,
        &dispatcher(),
        max_shapes,
        &mut || false,
    )
}

#[test]
fn expanding_one_folder_groups_its_small_children_and_changes_nothing_else() {
    let version = tree();
    let mut model = project(&version, SizeBasis::Logical, 16).unwrap();
    assert_eq!(model.shapes.len(), 2);
    let before = model.clone();
    expand(&mut model, &version, 0, 16).unwrap();
    finish_child_partitions(&model, &mut || false).unwrap();
    assert!(model.descendants.is_empty());
    assert_eq!(model.detail.groups.len(), 1);
    let group = &model.detail.groups[0];
    assert_eq!(group.members.len(), 100);
    assert_eq!(bytes_of(&group.total), 1_000);
    let folder = &model.shapes[0];
    assert_eq!(group.parent, model.rows[folder.row_index].item);
    assert_eq!(
        group.meta.parent_shape_ref.as_ref(),
        Some(&folder.meta.shape_ref)
    );
    assert_eq!(group.meta.visible_depth, 2);
    assert!(group.meta.shape_ref.starts_with('g'));
    assert_eq!(model.shape_count(), 3);
    // Root shapes, the other branch, the four totals and the scope are untouched.
    assert_eq!(model.shapes, before.shapes);
    assert_eq!(model.totals, before.totals);
    assert_eq!(
        model.rows[model.shapes[1].row_index],
        before.rows[before.shapes[1].row_index]
    );
    assert_eq!(before.rows[folder.row_index].omitted_children, 100);
    // The members are in the group, no longer hidden children of the folder.
    assert_eq!(model.rows[folder.row_index].omitted_children, 0);
    // The other folder gets its own group in its own box.
    expand(&mut model, &version, 1, 16).unwrap();
    assert_eq!(model.detail.groups.len(), 2);
    assert_eq!(model.totals, before.totals);
    assert_eq!(model.shapes, before.shapes);
}

#[test]
fn without_group_tail_an_expanded_tail_is_only_counted() {
    let version = tree();
    let mut model = project_with(&version, SizeBasis::Logical, 16, false).unwrap();
    expand(&mut model, &version, 0, 16).unwrap();
    assert!(model.descendants.is_empty() && model.detail.groups.is_empty());
    assert_eq!(model.shape_count(), 2);
    let folder = &model.shapes[0];
    assert_eq!(model.rows[folder.row_index].omitted_children, 100);
}

#[test]
fn a_level_with_no_remaining_slot_stays_hidden_behind_its_count() {
    let version = tree();
    let mut model = project(&version, SizeBasis::Logical, 16).unwrap();
    let before = model.clone();
    let limit = model.shape_count();
    expand(&mut model, &version, 0, limit).unwrap();
    assert_eq!(model, before);
}

#[test]
fn nested_levels_never_push_the_model_past_the_limit() {
    let mut entries = Vec::new();
    for folder in ["a", "b", "c"] {
        for i in 0..60 {
            entries.push((format!("{folder}/f{i:03}"), (i % 7 + 1) * 100));
        }
    }
    let version = build(&entries, ResultCompleteness::Complete);
    for limit in [3, 4, 6, 9, 40] {
        let mut model = project(&version, SizeBasis::Logical, limit).unwrap();
        for shape in 0..model.shapes.len() {
            expand(&mut model, &version, shape, limit).unwrap();
            assert!(model.shape_count() <= limit, "limit {limit}");
        }
        finish_child_partitions(&model, &mut || false).unwrap();
    }
}

#[test]
fn unattributed_children_of_an_expanded_folder_are_counted_as_unknown() {
    let sink = IngestSink::default();
    let mut builder = sink.open(id(1)).unwrap();
    for path in [
        vec![],
        vec![b"p".to_vec()],
        vec![b"p".to_vec(), b"x".to_vec()],
        vec![b"p".to_vec(), b"y".to_vec()],
    ] {
        builder.add_directory(path).unwrap();
    }
    for folder in [b"x", b"y"] {
        builder
            .add_with_space(
                Observation {
                    path: vec![b"p".to_vec(), folder.to_vec(), b"file".to_vec()],
                    size: Some(logical(10)),
                },
                observation(7, 2),
            )
            .unwrap();
    }
    let version = builder.seal(id(2), ResultCompleteness::Complete).unwrap();
    let mut model = project(&version, SizeBasis::Allocated, 16).unwrap();
    assert_eq!(model.shapes.len(), 1);
    expand(&mut model, &version, 0, 16).unwrap();
    assert_eq!(model.detail.unknown_items, 2);
    assert!(model.descendants.is_empty() && model.detail.groups.is_empty());
    assert_eq!(model.rows[0].omitted_children, 0);
}

#[test]
fn a_cancelled_expansion_adds_nothing() {
    let version = tree();
    let mut model = project(&version, SizeBasis::Logical, 16).unwrap();
    let before = model.clone();
    let result = add_child_partition(&mut model, &version, 0, &dispatcher(), 16, &mut || true);
    assert_eq!(result, Err(projection::Error::Cancelled));
    assert_eq!(model, before);
}
