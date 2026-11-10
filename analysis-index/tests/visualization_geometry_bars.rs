use du_analysis_index::visualization::{
    bar::{self, BarRef, Item as BarItem, Segment},
    geometry::{Rect, Scale, Weight},
    histogram::{self, Basis, Bin, Item as BinItem},
    waterfall::{self, Change, Direction},
};
use du_core_contracts::{
    result::ResultCompleteness,
    visualization::geometry::{COORDINATE_MAX, Geometry, GeometryError, Point},
};

fn bounds() -> Rect {
    Rect {
        left: 0,
        top: 0,
        right: COORDINATE_MAX,
        bottom: COORDINATE_MAX,
    }
}
fn assert_rect(output: &Option<Geometry>, source: Option<Rect>) {
    assert_eq!(output.is_some(), source.is_some());
    if let (Some(output), Some(rect)) = (output, source) {
        assert_eq!(output.primitive(), "rect");
        assert_eq!(
            output.points(),
            &[
                Point {
                    x: rect.left,
                    y: rect.top
                },
                Point {
                    x: rect.right,
                    y: rect.bottom
                }
            ]
        );
    }
}

#[test]
fn ranking_and_stack_preserve_references_legend_order_and_missing_slots() -> Result<(), String> {
    let input = [
        BarItem {
            item_no: 3,
            value: Weight::Exact(0),
        },
        BarItem {
            item_no: 1,
            value: Weight::Exact(20),
        },
        BarItem {
            item_no: 2,
            value: Weight::Unknown,
        },
    ];
    let ranking = bar::ranking(
        bounds(),
        &input,
        2,
        Scale::Fixed(20),
        ResultCompleteness::Complete,
    )
    .map_err(|e| format!("{e:?}"))?;
    let geometry = ranking.geometry_v1().map_err(|e| format!("{e:?}"))?;
    assert_eq!(geometry.len(), 2);
    assert_eq!(ranking.bars[0].reference, BarRef::Item(1));
    assert_eq!(ranking.bars[1].reference, BarRef::Item(3));
    assert_eq!(ranking.unknown_items, vec![2]);
    for (shape, row) in geometry.iter().zip(&ranking.bars) {
        assert_rect(shape, row.rect);
    }
    assert!(geometry[0].is_some());
    assert!(geometry[1].is_none());
    let stack = bar::stacked(
        bounds(),
        20,
        &[8, 3, 1],
        &[
            Segment {
                legend_key: 1,
                value: Weight::Exact(2),
            },
            Segment {
                legend_key: 8,
                value: Weight::Exact(5),
            },
            Segment {
                legend_key: 3,
                value: Weight::Unknown,
            },
        ],
    )
    .map_err(|e| format!("{e:?}"))?;
    let geometry = stack.geometry_v1().map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        stack
            .segments
            .iter()
            .map(|s| s.legend_key)
            .collect::<Vec<_>>(),
        vec![8, 3, 1]
    );
    assert_eq!(geometry.len(), 3);
    for (shape, segment) in geometry.iter().zip(&stack.segments) {
        assert_rect(shape, segment.rect);
    }
    assert!(geometry[0].is_some());
    assert!(geometry[1].is_none());
    assert!(geometry[2].is_none());
    assert_eq!(stack.segments[2].value, Weight::Exact(2));
    Ok(())
}

#[test]
fn histogram_preserves_empty_bins_counts_and_uncertain_items() -> Result<(), String> {
    let bins = [
        Bin {
            lower: 0,
            upper: Some(10),
        },
        Bin {
            lower: 10,
            upper: Some(20),
        },
        Bin {
            lower: 20,
            upper: None,
        },
    ];
    let input = [
        BinItem {
            item_no: 1,
            value: Weight::Exact(2),
        },
        BinItem {
            item_no: 2,
            value: Weight::Unknown,
        },
    ];
    let hist = histogram::layout(
        bounds(),
        &bins,
        &input,
        Basis::ItemCount,
        Scale::Fixed(2),
        ResultCompleteness::Complete,
    )
    .map_err(|e| format!("{e:?}"))?;
    let geometry = hist.geometry_v1().map_err(|e| format!("{e:?}"))?;
    assert_eq!(geometry.len(), bins.len());
    assert_eq!(hist.uncertain_items, vec![2]);
    for (shape, bin) in geometry.iter().zip(&hist.bins) {
        assert_rect(shape, bin.rect);
    }
    assert!(geometry[0].is_some());
    assert!(geometry[1].is_none());
    assert!(geometry[2].is_none());
    let empty = histogram::layout(
        bounds(),
        &bins,
        &[],
        Basis::Bytes,
        Scale::Fixed(10),
        ResultCompleteness::Complete,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        empty.geometry_v1().map_err(|e| format!("{e:?}"))?,
        vec![None; 3]
    );
    Ok(())
}

#[test]
fn waterfall_preserves_order_sign_and_missing_offsets() -> Result<(), String> {
    let changes = [
        Change {
            item_no: 1,
            direction: Direction::Increase,
            value: Weight::Exact(5),
        },
        Change {
            item_no: 2,
            direction: Direction::Decrease,
            value: Weight::Unknown,
        },
        Change {
            item_no: 3,
            direction: Direction::Decrease,
            value: Weight::Exact(2),
        },
        Change {
            item_no: 4,
            direction: Direction::Increase,
            value: Weight::Exact(0),
        },
    ];
    let result = waterfall::layout(
        bounds(),
        Weight::Exact(10),
        &changes,
        Weight::Exact(9),
        Scale::Fixed(20),
    )
    .map_err(|e| format!("{e:?}"))?;
    let geometry = result.geometry_v1().map_err(|e| format!("{e:?}"))?;
    assert_rect(&geometry.start, result.start.rect);
    assert_rect(&geometry.end, result.end.rect);
    assert_eq!(geometry.changes.len(), changes.len());
    for (shape, change) in geometry.changes.iter().zip(&result.changes) {
        assert_rect(shape, change.rect);
    }
    assert!(geometry.changes[0].is_some());
    assert!(geometry.changes[1..].iter().all(Option::is_none));
    assert_eq!(result.changes[1].direction, Some(Direction::Decrease));
    assert_eq!(result.changes[2].value, Weight::Exact(2));
    assert_eq!(result.uncertain_changes, vec![changes[1]]);
    Ok(())
}

#[test]
fn all_adapters_reject_drawn_out_of_grid_coordinates_without_clamping() -> Result<(), String> {
    let bounds = Rect {
        right: COORDINATE_MAX + 1,
        ..bounds()
    };
    let ranking = bar::ranking(
        bounds,
        &[BarItem {
            item_no: 1,
            value: Weight::Exact(10),
        }],
        1,
        Scale::Fixed(10),
        ResultCompleteness::Complete,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        ranking.geometry_v1().err(),
        Some(GeometryError::CoordinateOutOfRange)
    );
    let stack = bar::stacked(
        bounds,
        10,
        &[1],
        &[Segment {
            legend_key: 1,
            value: Weight::Exact(10),
        }],
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        stack.geometry_v1().err(),
        Some(GeometryError::CoordinateOutOfRange)
    );
    let hist = histogram::layout(
        bounds,
        &[Bin {
            lower: 0,
            upper: None,
        }],
        &[BinItem {
            item_no: 1,
            value: Weight::Exact(10),
        }],
        Basis::Bytes,
        Scale::Fixed(10),
        ResultCompleteness::Complete,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        hist.geometry_v1().err(),
        Some(GeometryError::CoordinateOutOfRange)
    );
    let fall = waterfall::layout(
        bounds,
        Weight::Exact(1),
        &[],
        Weight::Exact(1),
        Scale::Fixed(1),
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        fall.geometry_v1().err(),
        Some(GeometryError::CoordinateOutOfRange)
    );
    Ok(())
}

#[test]
fn waterfall_draws_known_increase_and_decrease_without_losing_direction() -> Result<(), String> {
    let changes = [
        Change {
            item_no: 11,
            direction: Direction::Increase,
            value: Weight::Exact(4),
        },
        Change {
            item_no: 12,
            direction: Direction::Decrease,
            value: Weight::Exact(3),
        },
    ];
    let output = waterfall::layout(
        bounds(),
        Weight::Exact(10),
        &changes,
        Weight::Exact(11),
        Scale::Automatic,
    )
    .map_err(|e| format!("{e:?}"))?;
    let geometry = output.geometry_v1().map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        output
            .changes
            .iter()
            .map(|bar| bar.direction)
            .collect::<Vec<_>>(),
        vec![Some(Direction::Increase), Some(Direction::Decrease)]
    );
    assert!(geometry.changes.iter().all(Option::is_some));
    for (shape, bar) in geometry.changes.iter().zip(&output.changes) {
        assert_rect(shape, bar.rect);
    }
    Ok(())
}
