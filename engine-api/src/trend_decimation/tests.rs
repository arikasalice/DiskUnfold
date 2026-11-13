use super::*;

#[derive(Clone)]
struct Point {
    time: i64,
    value: Option<u64>,
    preserve: bool,
}

impl TrendPoint for Point {
    fn observed_at_millis(&self) -> i64 {
        self.time
    }

    fn exact_value(&self) -> Option<u64> {
        self.value
    }

    fn must_preserve(&self) -> bool {
        self.preserve
    }
}

fn point(time: i64, value: Option<u64>) -> Point {
    Point {
        time,
        value,
        preserve: false,
    }
}

fn run(points: &[Point], budget: usize, range: TimeRange) -> Result<Decimated, Error> {
    decimate(
        points,
        &vec![false; points.len().saturating_sub(1)],
        budget,
        range,
    )
}

#[test]
fn validates_budget_even_when_no_decimation_is_needed() -> Result<(), Error> {
    let points = [point(0, Some(1))];
    let range = TimeRange {
        start_millis: 0,
        end_millis: 0,
    };
    assert_eq!(run(&points, 0, range), Err(Error::InvalidBudget));
    assert_eq!(run(&points, 1, range), Err(Error::InvalidBudget));
    assert_eq!(run(&points, 2001, range), Err(Error::InvalidBudget));
    assert_eq!(run(&points, 2, range)?.slot_indices, vec![0]);
    assert_eq!(run(&points, 2000, range)?.slot_indices, vec![0]);
    Ok(())
}

#[test]
fn budget_and_count_boundaries_report_decimation_state() -> Result<(), Error> {
    let points = (0..2002)
        .map(|time| point(time, Some(time as u64)))
        .collect::<Vec<_>>();
    let range = TimeRange {
        start_millis: 0,
        end_millis: 2001,
    };
    let same = run(&points[..2], 2, range)?;
    assert_eq!(same.slot_indices, vec![0, 1]);
    assert!(!same.decimated);
    assert_eq!(same.original_count, 2);
    let reduced = run(&points[..3], 2, range)?;
    assert!(reduced.decimated);
    assert_eq!(reduced.original_count, 3);
    let max_budget = run(&points[..2001], 2000, range)?;
    assert!(max_budget.decimated);
    let boundary = run(&points[..2000], 2000, range)?;
    assert!(!boundary.decimated);
    assert_eq!(boundary.slot_indices.len(), 2000);
    Ok(())
}

#[test]
fn bucket_ties_keep_earlier_slot_for_minimum_and_maximum() -> Result<(), Error> {
    let points = [
        point(0, Some(5)),
        point(0, Some(5)),
        point(0, Some(9)),
        point(0, Some(9)),
        point(0, Some(1)),
        point(0, Some(1)),
    ];
    let result = run(
        &points,
        2,
        TimeRange {
            start_millis: 0,
            end_millis: 0,
        },
    )?;
    assert_eq!(result.slot_indices, vec![0, 2, 4, 5]);
    Ok(())
}

#[test]
fn first_and_last_refer_to_exact_observations_while_unknowns_are_preserved() -> Result<(), Error> {
    let points = [
        point(0, None),
        point(0, Some(5)),
        point(0, Some(1)),
        point(0, Some(9)),
        point(0, Some(6)),
        point(0, None),
    ];
    let result = run(
        &points,
        2,
        TimeRange {
            start_millis: 0,
            end_millis: 0,
        },
    )?;
    assert_eq!(result.slot_indices, vec![0, 1, 2, 3, 4, 5]);
    assert!(result.decimated);
    Ok(())
}

#[test]
fn mandatory_points_overlap_bucket_selection_without_reordering() -> Result<(), Error> {
    let mut points = [
        point(0, Some(8)),
        point(0, None),
        point(0, Some(1)),
        point(0, Some(5)),
    ];
    points[1].preserve = true;
    let result = run(
        &points,
        2,
        TimeRange {
            start_millis: 0,
            end_millis: 0,
        },
    )?;
    assert_eq!(result.slot_indices, vec![0, 1, 2, 3]);
    assert!(result.decimated);
    Ok(())
}

#[test]
fn non_monotonic_and_equal_times_keep_original_slot_order() -> Result<(), Error> {
    let points = [
        point(9, Some(9)),
        point(1, Some(1)),
        point(9, Some(8)),
        point(1, Some(2)),
    ];
    let result = run(
        &points,
        2,
        TimeRange {
            start_millis: 0,
            end_millis: 10,
        },
    )?;
    assert_eq!(result.slot_indices.first(), Some(&0));
    assert!(result.slot_indices.windows(2).all(|pair| pair[0] < pair[1]));
    Ok(())
}

#[test]
fn reentered_buckets_are_merged_and_unknown_spans_are_projected() -> Result<(), Error> {
    // With two buckets over [0, 4], slots 0, 2, 4 revisit bucket 0.
    let points = [
        point(0, Some(8)),
        point(3, Some(4)),
        point(1, Some(7)),
        point(2, Some(6)),
        point(0, Some(1)),
    ];
    let result = decimate(
        &points,
        &[false, true, false, false],
        2,
        TimeRange {
            start_millis: 0,
            end_millis: 4,
        },
    )?;
    // Bucket 0 revisits slots 2 and 4 but retains the global extrema and
    // first/last exact observations. Bucket 1 contributes slot 1.
    assert_eq!(result.slot_indices, vec![0, 1, 4]);
    assert_eq!(result.segment_unknown, vec![false, true]);
    assert_eq!(result.segment_original_point_count, vec![2, 4]);
    Ok(())
}

#[test]
fn negative_and_extreme_times_use_wide_integer_arithmetic() -> Result<(), Error> {
    let points = [
        point(i64::MIN, Some(0)),
        point(0, Some(5)),
        point(i64::MAX, Some(1)),
    ];
    let result = run(
        &points,
        2,
        TimeRange {
            start_millis: i64::MIN,
            end_millis: i64::MAX,
        },
    )?;
    assert_eq!(result.slot_indices, vec![0, 1, 2]);
    Ok(())
}

#[test]
fn projected_segment_is_unknown_if_any_original_segment_is_unknown() -> Result<(), Error> {
    let points = [
        point(0, Some(0)),
        point(0, Some(1)),
        point(0, Some(2)),
        point(0, Some(3)),
    ];
    let result = decimate(
        &points,
        &[false, true, false],
        2,
        TimeRange {
            start_millis: 0,
            end_millis: 0,
        },
    )?;
    assert_eq!(result.slot_indices, vec![0, 3]);
    assert_eq!(result.segment_unknown, vec![true]);
    assert_eq!(result.segment_original_point_count, vec![4]);
    let all_unknown = decimate(
        &points,
        &[true, true, true],
        2,
        TimeRange {
            start_millis: 0,
            end_millis: 0,
        },
    )?;
    assert_eq!(all_unknown.segment_unknown, vec![true]);
    Ok(())
}

#[test]
fn validates_range_and_segment_shape() {
    let points = [point(0, Some(1)), point(2, Some(2)), point(1, Some(3))];
    assert_eq!(
        run(
            &points,
            2,
            TimeRange {
                start_millis: 2,
                end_millis: 1
            }
        ),
        Err(Error::InvalidRange)
    );
    assert_eq!(
        run(
            &points,
            2,
            TimeRange {
                start_millis: 0,
                end_millis: 1
            }
        ),
        Err(Error::PointOutsideRange)
    );
    assert_eq!(
        decimate(
            &points,
            &[],
            2,
            TimeRange {
                start_millis: 0,
                end_millis: 2
            }
        ),
        Err(Error::InvalidSegmentCount)
    );
}

#[test]
fn observation_time_is_read_once_before_bucket_assignment() -> Result<(), Error> {
    use std::cell::Cell;

    struct ChangingTime {
        reads: Cell<usize>,
    }

    impl TrendPoint for ChangingTime {
        fn observed_at_millis(&self) -> i64 {
            let read = self.reads.get();
            self.reads.set(read + 1);
            if read == 0 { 0 } else { i64::MAX }
        }
        fn exact_value(&self) -> Option<u64> {
            Some(1)
        }
        fn must_preserve(&self) -> bool {
            false
        }
    }

    let points = [
        ChangingTime {
            reads: Cell::new(0),
        },
        ChangingTime {
            reads: Cell::new(0),
        },
        ChangingTime {
            reads: Cell::new(0),
        },
    ];
    let result = decimate(
        &points,
        &[false, false],
        2,
        TimeRange {
            start_millis: 0,
            end_millis: 0,
        },
    )?;
    assert_eq!(result.slot_indices, vec![0, 2]);
    assert!(points.iter().all(|point| point.reads.get() == 1));
    Ok(())
}
