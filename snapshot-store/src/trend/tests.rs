use super::*;

fn time(value: &str) -> Result<i64, &'static str> {
    Ok(match value {
        "2025-01-01T00:00:00.000Z" => 1_735_689_600_000,
        "2025-01-02T00:00:00.000Z" => 1_735_776_000_000,
        "2025-01-03T00:00:00.000Z" => 1_735_862_400_000,
        "2025-01-04T00:00:00.000Z" => 1_735_948_800_000,
        "2025-01-05T00:00:00.000Z" => 1_736_035_200_000,
        "2025-01-06T00:00:00.000Z" => 1_736_121_600_000,
        _ => return Err("test timestamp is not in the fixed fixture set"),
    })
}

fn point(at: &str, evidence: PointEvidence) -> Result<Point, &'static str> {
    Ok(Point {
        observed_at_millis: time(at)?,
        evidence,
    })
}

fn millis(value: i64, evidence: PointEvidence) -> Point {
    Point {
        observed_at_millis: value,
        evidence,
    }
}

fn evaluation() -> Result<i64, &'static str> {
    time("2025-01-04T00:00:00.000Z")
}

#[test]
fn first_match_exact_including_zero_and_absence() -> Result<(), Box<dyn std::error::Error>> {
    let result = segment(
        &[
            point("2025-01-01T00:00:00.000Z", PointEvidence::Exact(0))?,
            point("2025-01-02T00:00:00.000Z", PointEvidence::Exact(1))?,
            point("2025-01-03T00:00:00.000Z", PointEvidence::AbsentComplete)?,
        ],
        evaluation()?,
    );
    let Some(Trend::Segments(rows)) = result else {
        return Err("expected adjacent segments".into());
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].delta_bytes, Some(1));
    assert_eq!(rows[0].status, SegmentStatus::Comparable);
    assert_eq!(rows[0].interval_millis, 86_400_000);
    assert_eq!(rows[0].before.slot_index, 0);
    assert_eq!(rows[0].after.slot_index, 1);
    assert_eq!(rows[1].after.value, PointValue::Observed(0));
    assert_eq!(rows[1].delta_bytes, Some(-1));
    assert_eq!(rows[1].after.slot_index, 2);
    Ok(())
}

#[test]
fn present_unknown_and_lower_bound_are_size_unknown() -> Result<(), Box<dyn std::error::Error>> {
    for evidence in [PointEvidence::LowerBound, PointEvidence::UnknownSize] {
        let result = segment(
            &[point("2025-01-01T00:00:00.000Z", evidence)?],
            evaluation()?,
        );
        assert!(matches!(
            result,
            Some(Trend::SingleObservation(PointResult {
                value: PointValue::SizeUnknown,
                ..
            }))
        ));
    }
    Ok(())
}

#[test]
fn unresolved_absence_and_f08_reasons_keep_their_reasons() -> Result<(), Box<dyn std::error::Error>>
{
    for (evidence, expected) in [
        (PointEvidence::Missing, Gap::MissingObservation),
        (PointEvidence::OutsideScope, Gap::OutsideScope),
        (PointEvidence::Excluded, Gap::Excluded),
        (PointEvidence::AccessDenied, Gap::AccessDenied),
        (
            PointEvidence::Gap(Gap::DifferentStorage),
            Gap::DifferentStorage,
        ),
    ] {
        let result = segment(
            &[point("2025-01-01T00:00:00.000Z", evidence)?],
            evaluation()?,
        );
        assert!(
            matches!(result, Some(Trend::SingleObservation(PointResult { value: PointValue::Unverifiable(gap), .. })) if gap == expected)
        );
    }
    Ok(())
}

#[test]
fn point_values_expose_only_published_reason_keys() {
    for (value, expected) in [
        (PointValue::SizeUnknown, Some("trend.point.size_unknown")),
        (
            PointValue::Unverifiable(Gap::OutsideScope),
            Some("trend.point.out_of_scope"),
        ),
        (
            PointValue::Unverifiable(Gap::Excluded),
            Some("trend.point.excluded"),
        ),
        (
            PointValue::Unverifiable(Gap::AccessDenied),
            Some("trend.point.denied"),
        ),
        (
            PointValue::Unverifiable(Gap::MissingObservation),
            Some("trend.point.missing"),
        ),
        (PointValue::Unverifiable(Gap::DifferentStorage), None),
        (PointValue::Observed(0), None),
    ] {
        assert_eq!(value.reason_key(), expected);
    }
}

#[test]
fn single_point_has_no_interpolated_segment() -> Result<(), Box<dyn std::error::Error>> {
    let result = segment(
        &[point("2025-01-01T00:00:00.000Z", PointEvidence::Exact(4))?],
        evaluation()?,
    );
    assert!(matches!(result, Some(Trend::SingleObservation(_))));
    assert!(matches!(
        result,
        Some(Trend::SingleObservation(PointResult { slot_index: 0, .. }))
    ));
    assert_eq!(segment(&[], evaluation()?), None);
    Ok(())
}

#[test]
fn future_observation_marks_order_uncertain_and_disables_rate()
-> Result<(), Box<dyn std::error::Error>> {
    let result = segment(
        &[
            point("2025-01-01T00:00:00.000Z", PointEvidence::Exact(1))?,
            point("2025-01-05T00:00:00.000Z", PointEvidence::Exact(3))?,
        ],
        evaluation()?,
    );
    let Some(Trend::Segments(rows)) = result else {
        return Err("expected segment".into());
    };
    assert_eq!(rows[0].after.flags, vec![Flag::OrderUncertain]);
    assert_eq!(rows[0].delta_bytes, Some(2));
    assert_eq!(rows[0].bytes_per_day, None);
    Ok(())
}

#[test]
fn future_start_point_also_disables_rate() -> Result<(), Box<dyn std::error::Error>> {
    let result = segment(
        &[
            point("2025-01-05T00:00:00.000Z", PointEvidence::Exact(3))?,
            point("2025-01-06T00:00:00.000Z", PointEvidence::Exact(5))?,
        ],
        evaluation()?,
    );
    let Some(Trend::Segments(rows)) = result else {
        return Err("expected segment".into());
    };
    assert_eq!(rows[0].before.flags, vec![Flag::OrderUncertain]);
    assert_eq!(rows[0].before.slot_index, 0);
    assert_eq!(rows[0].after.slot_index, 1);
    assert_eq!(rows[0].status, SegmentStatus::Comparable);
    assert_eq!(rows[0].delta_bytes, Some(2));
    assert_eq!(rows[0].bytes_per_day, None);
    Ok(())
}

#[test]
fn future_prior_snapshot_then_appended_current_preserves_slots_and_delta()
-> Result<(), Box<dyn std::error::Error>> {
    let result = segment(
        &[
            point("2025-01-05T00:00:00.000Z", PointEvidence::Exact(3))?,
            point("2025-01-03T00:00:00.000Z", PointEvidence::Exact(8))?,
        ],
        evaluation()?,
    );
    let Some(Trend::Segments(rows)) = result else {
        return Err("expected segment".into());
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].before.slot_index, 0);
    assert_eq!(rows[0].after.slot_index, 1);
    assert_eq!(rows[0].before.flags, vec![Flag::OrderUncertain]);
    assert!(rows[0].after.flags.is_empty());
    assert_eq!(rows[0].delta_bytes, Some(5));
    assert_eq!(rows[0].interval_millis, -172_800_000);
    assert_eq!(rows[0].bytes_per_day, None);
    Ok(())
}

#[test]
fn three_hundred_sixty_five_points_are_deterministic_without_synthetic_slots()
-> Result<(), Box<dyn std::error::Error>> {
    let points = (0..365)
        .map(|slot| millis(slot, PointEvidence::Exact(slot as u64)))
        .collect::<Vec<_>>();
    let first = segment(&points, i64::MAX).ok_or("expected trend for nonempty points")?;
    let second = segment(&points, i64::MAX).ok_or("expected repeated trend")?;
    assert_eq!(first, second);
    let Trend::Segments(rows) = first else {
        return Err("365 observations must produce adjacent segments".into());
    };
    assert_eq!(points.len(), 365);
    assert_eq!(rows.len(), 364);
    for (slot, row) in rows.iter().enumerate() {
        assert_eq!(row.before.slot_index, slot);
        assert_eq!(row.after.slot_index, slot + 1);
        assert_eq!(row.delta_bytes, Some(1));
        assert_eq!(row.interval_millis, 1);
    }
    Ok(())
}

#[test]
fn equal_and_reversed_times_are_preserved_without_rate() -> Result<(), Box<dyn std::error::Error>> {
    let result = segment(
        &[
            point("2025-01-02T00:00:00.000Z", PointEvidence::Exact(9))?,
            point("2025-01-02T00:00:00.000Z", PointEvidence::Exact(3))?,
            point("2025-01-01T00:00:00.000Z", PointEvidence::Exact(1))?,
        ],
        evaluation()?,
    );
    let Some(Trend::Segments(rows)) = result else {
        return Err("expected segments".into());
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].before.value, PointValue::Observed(9));
    assert_eq!(rows[0].after.value, PointValue::Observed(3));
    assert_eq!(rows[0].before.slot_index, 0);
    assert_eq!(rows[0].after.slot_index, 1);
    assert_eq!(rows[0].delta_bytes, Some(-6));
    assert_eq!(rows[0].bytes_per_day, None);
    assert_eq!(rows[1].after.value, PointValue::Observed(1));
    assert_eq!(rows[1].after.slot_index, 2);
    assert_eq!(rows[1].delta_bytes, Some(-2));
    assert_eq!(rows[1].bytes_per_day, None);
    Ok(())
}

#[test]
fn negative_rate_truncates_toward_zero() -> Result<(), Box<dyn std::error::Error>> {
    let result = segment(
        &[
            point("2025-01-01T00:00:00.000Z", PointEvidence::Exact(10))?,
            point("2025-01-03T00:00:00.000Z", PointEvidence::Exact(7))?,
        ],
        evaluation()?,
    );
    let Some(Trend::Segments(rows)) = result else {
        return Err("expected segment".into());
    };
    assert_eq!(rows[0].bytes_per_day, Some(-1));
    Ok(())
}

#[test]
fn unknown_endpoint_has_no_segment_delta_or_rate() -> Result<(), Box<dyn std::error::Error>> {
    let result = segment(
        &[
            point("2025-01-01T00:00:00.000Z", PointEvidence::Exact(10))?,
            point("2025-01-02T00:00:00.000Z", PointEvidence::LowerBound)?,
        ],
        evaluation()?,
    );
    let Some(Trend::Segments(rows)) = result else {
        return Err("expected segment".into());
    };
    assert_eq!(rows[0].delta_bytes, None);
    assert_eq!(rows[0].bytes_per_day, None);
    Ok(())
}

#[test]
fn arithmetic_at_supported_extremes_is_checked_and_deterministic()
-> Result<(), Box<dyn std::error::Error>> {
    let result = segment(
        &[
            millis(i64::MIN, PointEvidence::Exact(0)),
            millis(i64::MAX, PointEvidence::Exact(u64::MAX)),
        ],
        i64::MAX,
    );
    let Some(Trend::Segments(rows)) = result else {
        return Err("expected segment".into());
    };
    assert_eq!(rows[0].delta_bytes, Some(i128::from(u64::MAX)));
    assert_eq!(
        rows[0].interval_millis,
        i128::from(i64::MAX) - i128::from(i64::MIN)
    );
    assert_eq!(rows[0].bytes_per_day, Some(86_400_000));
    assert_eq!(
        segment(
            &[
                millis(i64::MIN, PointEvidence::Exact(0)),
                millis(i64::MAX, PointEvidence::Exact(u64::MAX)),
            ],
            i64::MAX,
        ),
        Some(Trend::Segments(rows.clone()))
    );
    Ok(())
}

#[test]
fn rate_overflow_returns_none() {
    assert_eq!(rate(Some(i128::MAX), 1, false), None);
    assert_eq!(rate(Some(-3), 172_800_000, false), Some(-1));
}
