use du_analysis_index::visualization::{
    geometry::{Rect, Weight},
    squarify,
    trend::{self, Point, Series},
};
use du_core_contracts::visualization::geometry::{
    COORDINATE_MAX, GeometryError, Point as WirePoint,
};

fn bounds() -> Rect {
    Rect {
        left: 0,
        top: 0,
        right: COORDINATE_MAX,
        bottom: COORDINATE_MAX,
    }
}

#[test]
fn actual_treemap_layout_produces_rectangles_with_source_indices() -> Result<(), String> {
    let layout = squarify::layout(
        bounds(),
        &[Weight::Exact(3), Weight::Exact(1), Weight::Exact(0)],
    )
    .map_err(|e| format!("{e:?}"))?;
    let converted = layout.geometry_v1().map_err(|e| format!("{e:?}"))?;
    assert_eq!(converted.len(), 2);
    for ((index, geom), shape) in converted.iter().zip(&layout.shapes) {
        assert_eq!(*index, shape.input_index);
        assert_eq!(geom.primitive(), "rect");
        assert_eq!(
            geom.points(),
            &[
                WirePoint {
                    x: shape.rect.left,
                    y: shape.rect.top
                },
                WirePoint {
                    x: shape.rect.right,
                    y: shape.rect.bottom
                }
            ]
        );
    }
    assert_eq!(layout.skipped.len(), 1);
    Ok(())
}

#[test]
fn actual_trend_keeps_gaps_and_observation_indices() -> Result<(), String> {
    let points = [
        (0, Weight::Exact(0)),
        (1, Weight::Exact(10)),
        (2, Weight::Unknown),
        (3, Weight::Exact(30)),
        (4, Weight::Exact(40)),
    ]
    .into_iter()
    .map(|(observed_at_ms, value)| Point {
        observed_at_ms,
        value,
    })
    .collect();
    let output = trend::layout(
        bounds(),
        &[Series {
            series_no: 3,
            points,
        }],
    )
    .map_err(|e| format!("{e:?}"))?;
    let source = &output.series[0];
    let converted = source.geometry_v1().map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        converted
            .points
            .iter()
            .map(|(index, _)| *index)
            .collect::<Vec<_>>(),
        vec![0, 1, 3, 4]
    );
    assert_eq!(converted.lines.len(), 2);
    for (line, source) in converted.lines.iter().zip(&source.segments) {
        assert_eq!(line.primitive(), "polyline");
        assert_eq!(
            line.points(),
            &[
                WirePoint {
                    x: source.from.x,
                    y: source.from.y
                },
                WirePoint {
                    x: source.to.x,
                    y: source.to.y
                }
            ]
        );
    }
    assert!(
        converted
            .points
            .iter()
            .all(|(_, point)| point.primitive() == "point")
    );
    Ok(())
}

#[test]
fn adapters_reject_coordinates_outside_normalized_grid() -> Result<(), String> {
    let out_of_grid = Rect {
        right: COORDINATE_MAX + 1,
        ..bounds()
    };
    let rectangles =
        squarify::layout(out_of_grid, &[Weight::Exact(1)]).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        rectangles.geometry_v1().err(),
        Some(GeometryError::CoordinateOutOfRange)
    );
    let points = vec![
        Point {
            observed_at_ms: 0,
            value: Weight::Exact(1),
        },
        Point {
            observed_at_ms: 1,
            value: Weight::Exact(2),
        },
    ];
    let trend = trend::layout(
        out_of_grid,
        &[Series {
            series_no: 1,
            points,
        }],
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        trend.series[0].geometry_v1().err(),
        Some(GeometryError::CoordinateOutOfRange)
    );
    Ok(())
}

#[test]
fn admitted_treemap_partition_uses_same_geometry_adapter() -> Result<(), String> {
    use du_analysis_index::{
        IngestSink, Observation,
        visualization::treemap::{self, ChildKey},
    };
    use du_core_contracts::{
        Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
        result::{ResultCompleteness, ResultVersionRead},
    };
    let id = |n| Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"));
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_file(Observation {
            path: vec![b"a".to_vec()],
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: SizeAmount::Exact {
                    bytes: 5_u64.into(),
                },
            }),
        })
        .map_err(|e| format!("{e:?}"))?;
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let item = |item_no| ItemRef {
        result_version: version.version_id().clone(),
        item_no,
    };
    let keys = [ChildKey {
        item: item(1),
        path_compare_key: b"a".to_vec(),
    }];
    let partition = treemap::layout_parent(&version, &item(0), SizeBasis::Logical, bounds(), &keys)
        .map_err(|e| format!("{e:?}"))?;
    let geometry = partition
        .layout
        .geometry_v1()
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(geometry.len(), 1);
    assert_eq!(partition.children[geometry[0].0].item, item(1));
    assert_eq!(geometry[0].1.primitive(), "rect");
    Ok(())
}
