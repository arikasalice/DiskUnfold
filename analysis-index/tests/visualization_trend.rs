#![allow(clippy::unwrap_used)]

use du_analysis_index::visualization::{
    geometry::{Rect, Weight},
    trend::{self, Coordinate, Point, Series},
};

fn point(observed_at_ms: i64, value: Weight) -> Point {
    Point {
        observed_at_ms,
        value,
    }
}

fn bounds() -> Rect {
    Rect {
        left: 10,
        top: 20,
        right: 110,
        bottom: 120,
    }
}

#[test]
fn preserves_actual_spacing_and_adds_no_points() {
    let result = trend::layout(
        bounds(),
        &[Series {
            series_no: 7,
            points: vec![
                point(1_000, Weight::Exact(0)),
                point(1_100, Weight::Exact(50)),
                point(2_000, Weight::Exact(100)),
            ],
        }],
    )
    .unwrap();

    let plotted = &result.series[0];
    assert_eq!(plotted.samples.len(), 3);
    assert_eq!(plotted.samples[0].coordinate.unwrap().x, 10);
    assert_eq!(plotted.samples[1].coordinate.unwrap().x, 20);
    assert_eq!(plotted.samples[2].coordinate.unwrap().x, 110);
    assert_eq!(plotted.samples[0].coordinate.unwrap().y, 120);
    assert_eq!(plotted.samples[1].coordinate.unwrap().y, 70);
    assert_eq!(plotted.samples[2].coordinate.unwrap().y, 20);
    assert_eq!(plotted.segments.len(), 2);
}

#[test]
fn unknown_observations_break_lines_without_becoming_zero() {
    let result = trend::layout(
        bounds(),
        &[Series {
            series_no: 1,
            points: vec![
                point(10, Weight::Exact(10)),
                point(20, Weight::Unknown),
                point(30, Weight::Exact(30)),
                point(40, Weight::Exact(40)),
            ],
        }],
    )
    .unwrap();

    let plotted = &result.series[0];
    assert_eq!(plotted.samples[1].value, Weight::Unknown);
    assert_eq!(plotted.samples[1].x, 43);
    assert_eq!(plotted.samples[1].coordinate, None);
    assert_eq!(plotted.segments.len(), 1);
    assert_eq!(
        plotted.segments[0].from,
        plotted.samples[2].coordinate.unwrap()
    );
    assert_eq!(
        plotted.segments[0].to,
        plotted.samples[3].coordinate.unwrap()
    );
}

#[test]
fn aligns_multiple_series_to_one_domain_and_preserves_input_order() {
    let result = trend::layout(
        bounds(),
        &[
            Series {
                series_no: 9,
                points: vec![point(0, Weight::LowerBound(20))],
            },
            Series {
                series_no: 3,
                points: vec![point(100, Weight::Exact(40))],
            },
        ],
    )
    .unwrap();

    assert_eq!(result.series[0].series_no, 9);
    assert_eq!(result.series[1].series_no, 3);
    assert_eq!(result.series[0].samples[0].coordinate.unwrap().x, 10);
    assert_eq!(result.series[1].samples[0].coordinate.unwrap().x, 110);
    assert_eq!(result.value_range.unwrap().minimum, 20);
    assert_eq!(result.value_range.unwrap().maximum, 40);
}

#[test]
fn constant_domains_are_centered_and_empty_input_has_no_ranges() {
    let constant = trend::layout(
        bounds(),
        &[Series {
            series_no: 1,
            points: vec![point(50, Weight::Exact(8))],
        }],
    )
    .unwrap();
    assert_eq!(
        constant.series[0].samples[0].coordinate.unwrap(),
        Coordinate { x: 60, y: 70 }
    );

    let empty = trend::layout(bounds(), &[]).unwrap();
    assert_eq!(empty.time_range_ms, None);
    assert_eq!(empty.value_range, None);
    assert!(empty.series.is_empty());
}

#[test]
fn validates_bounds_series_identity_and_strict_time_order() {
    let invalid = Rect {
        right: 10,
        left: 10,
        top: 0,
        bottom: 1,
    };
    assert_eq!(
        trend::layout(invalid, &[]),
        Err(trend::Error::InvalidBounds)
    );

    let duplicate = [
        Series {
            series_no: 2,
            points: vec![],
        },
        Series {
            series_no: 2,
            points: vec![],
        },
    ];
    assert_eq!(
        trend::layout(bounds(), &duplicate),
        Err(trend::Error::DuplicateSeries)
    );

    let unordered = [Series {
        series_no: 1,
        points: vec![point(2, Weight::Exact(1)), point(2, Weight::Exact(3))],
    }];
    assert_eq!(
        trend::layout(bounds(), &unordered),
        Err(trend::Error::NonIncreasingTime)
    );
}

#[test]
fn handles_full_integer_domains_without_overflow() {
    let result = trend::layout(
        Rect {
            left: 0,
            top: 0,
            right: u32::MAX,
            bottom: u32::MAX,
        },
        &[Series {
            series_no: 1,
            points: vec![
                point(i64::MIN, Weight::Exact(0)),
                point(i64::MAX, Weight::Exact(u64::MAX)),
            ],
        }],
    )
    .unwrap();
    let samples = &result.series[0].samples;
    assert_eq!(
        samples[0].coordinate.unwrap(),
        Coordinate { x: 0, y: u32::MAX }
    );
    assert_eq!(
        samples[1].coordinate.unwrap(),
        Coordinate { x: u32::MAX, y: 0 }
    );
}
