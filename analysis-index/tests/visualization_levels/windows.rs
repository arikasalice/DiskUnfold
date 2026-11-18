//! The shape limit and the lower-bound rule under any visible window and result state.
use super::*;
use du_analysis_index::visualization::projection::{add_child_partition, finish_child_partitions};

fn windows() -> [Rect; 4] {
    [
        WINDOW,
        // A zoomed window that is a part of the whole space.
        Rect {
            left: 400_000,
            top: 300_000,
            right: 600_000,
            bottom: 500_000,
        },
        Rect {
            left: 999_000,
            top: 999_500,
            right: 1_000_000,
            bottom: 1_000_000,
        },
        // A window so small that the integer grid cannot represent most siblings.
        Rect {
            left: 10,
            top: 10,
            right: 110,
            bottom: 110,
        },
    ]
}

fn inside(geometry: &du_core_contracts::visualization::geometry::Geometry, window: Rect) -> bool {
    let p = geometry.points();
    p[0].x >= window.left
        && p[0].y >= window.top
        && p[1].x <= window.right
        && p[1].y <= window.bottom
}

#[test]
fn a_zoomed_window_keeps_the_limit_and_every_shape_inside_the_window() {
    let version = build(&mixed_sizes(), ResultCompleteness::Complete);
    for window in windows() {
        for limit in [1, 3, 16, 200] {
            let model = project_in(&version, SizeBasis::Logical, limit, true, window).unwrap();
            assert!(model.shape_count() <= limit, "{window:?} limit {limit}");
            let all_inside = model
                .shapes
                .iter()
                .map(|shape| &shape.geometry)
                .chain(model.detail.groups.iter().map(|group| &group.geometry))
                .all(|geometry| inside(geometry, window));
            assert!(all_inside, "{window:?} limit {limit}");
            let drawn: u64 = model.shapes.iter().map(|s| area(&s.geometry)).sum::<u64>()
                + model
                    .detail
                    .groups
                    .iter()
                    .map(|g| area(&g.geometry))
                    .sum::<u64>();
            assert!(
                drawn
                    <= u64::from(window.right - window.left)
                        * u64::from(window.bottom - window.top)
            );
        }
    }
}

#[test]
fn the_split_depends_on_the_share_of_the_window_not_on_its_size() {
    let version = build(&mixed_sizes(), ResultCompleteness::Complete);
    let split = |window| {
        let model = project_in(&version, SizeBasis::Logical, 16, true, window).unwrap();
        let members = model.detail.groups.first().map_or(0, |g| g.members.len());
        (model.shapes.len(), members)
    };
    // Zooming changes the pixels per unit, not which siblings are large enough.
    assert_eq!(split(windows()[0]), split(windows()[1]));
    assert_eq!(split(windows()[0]), split(windows()[2]));
}

fn mixed_sizes() -> Vec<(String, u64)> {
    let mut entries = files(&[("big1", 4_000), ("big2", 3_000), ("big3", 2_000)]);
    entries.extend((0..100).map(|i| (format!("s{i:03}"), 10)));
    entries
}

#[test]
fn the_total_over_every_depth_stays_within_the_limit_in_any_window() {
    let mut entries = Vec::new();
    for folder in ["a", "b", "c"] {
        for i in 0..60 {
            entries.push((format!("{folder}/f{i:03}"), (i % 7 + 1) * 100));
        }
    }
    let version = build(&entries, ResultCompleteness::Complete);
    for window in windows() {
        for limit in [3, 4, 6, 9, 40] {
            let mut model = project_in(&version, SizeBasis::Logical, limit, true, window).unwrap();
            for shape in 0..model.shapes.len() {
                add_child_partition(
                    &mut model,
                    &version,
                    shape,
                    &dispatcher(),
                    limit,
                    &mut || false,
                )
                .unwrap();
                assert!(model.shape_count() <= limit, "{window:?} limit {limit}");
            }
            finish_child_partitions(&model, &mut || false).unwrap();
        }
    }
}

// A value that is only a lower bound is drawn by its bound, whether the result is
// complete or partial: the file without a size is counted and gets no area, and the scope
// is a lower bound. The allocated basis is still refused here because these files carry no
// allocated evidence, so the parent has no allocated value to partition.
#[test]
fn a_lower_bound_scope_is_drawn_by_its_bound_whatever_the_completeness() {
    for completeness in [ResultCompleteness::Partial, ResultCompleteness::Complete] {
        let sink = IngestSink::default();
        let mut builder = sink.open(id(1)).unwrap();
        builder.add_directory(vec![]).unwrap();
        for (name, size) in [("measured", Some(logical(25))), ("unmeasured", None)] {
            builder
                .add_file(Observation {
                    path: vec![name.as_bytes().to_vec()],
                    size,
                })
                .unwrap();
        }
        let version = builder.seal(id(2), completeness).unwrap();
        let model = project(&version, SizeBasis::Logical, 16).unwrap();
        assert!(matches!(
            model.totals.scope.amount,
            SizeAmount::LowerBound { .. }
        ));
        assert_eq!(model.shapes.len(), 1, "{completeness:?}");
        assert_eq!(model.detail.unknown_items, 1, "{completeness:?}");
        assert_eq!(
            project(&version, SizeBasis::Allocated, 16).err(),
            Some(projection::Error::UnavailableAttribution),
            "{completeness:?}"
        );
    }
}
