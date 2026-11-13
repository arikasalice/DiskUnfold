use du_analysis_index::visualization::{
    bundle,
    geometry::Rect,
    trend::{Point as LayoutPoint, Series},
    typed_layout::{TrendBreak, TrendInput},
};
use du_core_contracts::visualization::quantity::VizQuantity;
use du_snapshot_store::trend::{Flag, Point, PointEvidence, PointValue, Trend, segment};

#[test]
fn f27_segments_supply_f06_with_tied_points_and_unordered_intervals() -> Result<(), String> {
    let observations = [
        Point {
            observed_at_millis: 0,
            evidence: PointEvidence::Exact(0),
        },
        Point {
            observed_at_millis: 20,
            evidence: PointEvidence::Exact(10),
        },
        Point {
            observed_at_millis: 20,
            evidence: PointEvidence::Exact(15),
        },
        Point {
            observed_at_millis: 100,
            evidence: PointEvidence::Exact(50),
        },
        Point {
            observed_at_millis: 40,
            evidence: PointEvidence::Exact(20),
        },
    ];
    let Some(Trend::Segments(segments)) = segment(&observations, 80) else {
        return Err("expected F27 adjacent observations".into());
    };
    let first = &segments.first().ok_or("missing F27 segment")?.before;
    let points = std::iter::once(first)
        .chain(segments.iter().map(|segment| &segment.after))
        .map(|point| {
            let PointValue::Observed(bytes) = point.value else {
                return Err("expected exact F27 point");
            };
            Ok(LayoutPoint {
                observed_at_ms: point.observed_at_millis,
                value: VizQuantity::exact("bytes", i128::from(bytes))
                    .ok_or("invalid F27 quantity")?,
            })
        })
        .collect::<Result<_, _>>()?;
    let series = [Series {
        series_no: 1,
        points,
    }];
    let breaks: Vec<_> = segments
        .iter()
        .filter(|segment| {
            segment.interval_millis <= 0
                || [&segment.before, &segment.after]
                    .iter()
                    .any(|point| point.flags.contains(&Flag::OrderUncertain))
        })
        .map(|segment| TrendBreak {
            series_no: 1,
            to_sample: segment.after.slot_index,
        })
        .collect();
    let selected = bundle::default_dispatcher()
        .map_err(|error| format!("{error:?}"))?
        .execute_trend(
            None,
            TrendInput {
                bounds: Rect {
                    left: 0,
                    top: 0,
                    right: 100,
                    bottom: 100,
                },
                series: &series,
                breaks: &breaks,
            },
            &mut || false,
        )
        .map_err(|error| format!("{error:?}"))?;

    let series = &selected.output.series[0];
    assert_eq!(
        series
            .samples
            .iter()
            .map(|sample| sample.x)
            .collect::<Vec<_>>(),
        [0, 20, 20, 100, 40]
    );
    assert_eq!(series.samples.len(), observations.len());
    assert_eq!(series.segments.len(), 1);
    assert_eq!(
        (series.segments[0].from_sample, series.segments[0].to_sample),
        (0, 1)
    );
    Ok(())
}
