use du_analysis_index::visualization::{
    bundle,
    geometry::Rect,
    registry,
    trend::{self, Point, Series},
    typed_layout::{Dispatcher, Strategy, TrendDispatchError, TrendError, TrendInput},
};
use du_core_contracts::visualization::{
    LayoutStrategyKey,
    quantity::{VizAmount, VizQuantity},
};

const BOUNDS: Rect = Rect {
    left: 0,
    top: 0,
    right: 1_000,
    bottom: 100,
};

fn dispatcher() -> Result<Dispatcher, String> {
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    bundle::register_trend_candidate(&mut builder).map_err(|error| format!("{error:?}"))?;
    Ok(Dispatcher::new(
        builder.build(&[]).map_err(|error| format!("{error:?}"))?,
    ))
}

fn bytes(observed_at_ms: i64, value: i128) -> Result<Point<VizQuantity>, String> {
    Ok(Point {
        observed_at_ms,
        value: VizQuantity::exact("bytes", value).ok_or("invalid test quantity")?,
    })
}

#[test]
fn registered_trend_preserves_signed_extent_actual_time_and_missing_runs() -> Result<(), String> {
    let dispatcher = dispatcher()?;
    let limit = i128::from(u64::MAX);
    let source = [Series {
        series_no: 1,
        points: vec![
            bytes(-1_000, -limit)?,
            bytes(0, 0)?,
            Point {
                observed_at_ms: 1_000,
                value: VizQuantity::unknown("bytes", "trend.unobserved".into())
                    .ok_or("invalid test quantity")?,
            },
            bytes(9_000, limit)?,
        ],
    }];
    let result = dispatcher
        .execute_trend(
            None,
            TrendInput {
                bounds: BOUNDS,
                series: &source,
            },
            &mut || false,
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(result.descriptor.strategy.id, "viz.trend");
    assert_eq!(result.descriptor.strategy.revision, 1);
    assert_eq!(
        result.output.value_range,
        Some(trend::ValueRange {
            minimum: -limit,
            maximum: limit
        })
    );
    let series = &result.output.series[0];
    assert_eq!(
        series
            .samples
            .iter()
            .map(|sample| sample.x)
            .collect::<Vec<_>>(),
        [0, 100, 200, 1_000]
    );
    assert_eq!(series.samples[0].coordinate.map(|p| p.y), Some(100));
    assert_eq!(series.samples[1].coordinate.map(|p| p.y), Some(50));
    assert_eq!(series.samples[2].coordinate, None);
    assert!(
        matches!(series.samples[2].value.amount(), VizAmount::Unknown { reason_key } if reason_key == "trend.unobserved")
    );
    assert_eq!(series.samples[3].coordinate.map(|p| p.y), Some(0));
    assert_eq!(
        series
            .segments
            .iter()
            .map(|line| (line.from_sample, line.to_sample))
            .collect::<Vec<_>>(),
        [(0, 1)]
    );
    let geometry = series.geometry_v1().map_err(|error| format!("{error:?}"))?;
    assert_eq!(geometry.points.len(), 3);
    assert_eq!(geometry.lines.len(), 1);
    Ok(())
}

#[test]
fn trend_requires_two_observed_times_and_rejects_non_byte_rows() -> Result<(), String> {
    let dispatcher = dispatcher()?;
    let source = [
        Series {
            series_no: 1,
            points: vec![bytes(7, 10)?],
        },
        Series {
            series_no: 2,
            points: vec![bytes(7, 11)?],
        },
    ];
    assert!(matches!(
        dispatcher.execute_trend(
            None,
            TrendInput {
                bounds: BOUNDS,
                series: &source
            },
            &mut || false
        ),
        Err(TrendDispatchError::Trend(
            TrendError::InsufficientObservations
        ))
    ));
    let source = [Series {
        series_no: 1,
        points: vec![
            bytes(1, 10)?,
            Point {
                observed_at_ms: 2,
                value: VizQuantity::exact("items", 1).ok_or("invalid test quantity")?,
            },
        ],
    }];
    assert!(matches!(
        dispatcher.execute_trend(
            None,
            TrendInput {
                bounds: BOUNDS,
                series: &source
            },
            &mut || false
        ),
        Err(TrendDispatchError::Trend(TrendError::InvalidUnit))
    ));
    Ok(())
}

#[test]
fn trend_dispatch_honors_exact_strategy_and_cancels_during_layout() -> Result<(), String> {
    let dispatcher = dispatcher()?;
    let source = [Series {
        series_no: 1,
        points: vec![bytes(0, 1)?, bytes(5, 2)?],
    }];
    let input = || TrendInput {
        bounds: BOUNDS,
        series: &source,
    };
    let missing = LayoutStrategyKey {
        id: "viz.trend".into(),
        revision: 2,
    };
    assert!(matches!(
        dispatcher.execute_trend(Some(&missing), input(), &mut || false),
        Err(TrendDispatchError::Selection(
            registry::SelectionError::UnknownStrategy
        ))
    ));
    for stop in [1, 5, 9] {
        let mut calls = 0;
        assert!(matches!(
            dispatcher.execute_trend(None, input(), &mut || {
                calls += 1;
                calls == stop
            }),
            Err(TrendDispatchError::Trend(TrendError::Layout(
                trend::Error::Cancelled
            )))
        ));
    }
    Ok(())
}
