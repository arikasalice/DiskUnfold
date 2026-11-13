use du_analysis_index::visualization::{
    bar::{self, BarRef, Error, Item, Segment},
    geometry::{NoArea, Rect, Scale, Weight},
};
use du_core_contracts::result::ResultCompleteness;

fn bounds() -> Rect {
    Rect {
        left: 10,
        top: 20,
        right: 110,
        bottom: 110,
    }
}
fn item(item_no: u32, value: Weight) -> Item {
    Item { item_no, value }
}
fn segment(legend_key: u32, value: Weight) -> Segment {
    Segment { legend_key, value }
}

#[test]
fn exact_and_lower_bound_zero_have_distinct_area_reasons() -> Result<(), Error> {
    let ranking = bar::ranking(
        bounds(),
        &[item(1, Weight::Exact(0)), item(2, Weight::LowerBound(0))],
        2,
        Scale::Fixed(10),
        ResultCompleteness::Complete,
    )?;
    assert_eq!(ranking.bars[0].no_area, Some(NoArea::ExactZero));
    assert_eq!(ranking.bars[1].no_area, Some(NoArea::LowerBoundZero));
    assert!(ranking.bars.iter().all(|row| row.rect.is_none()));
    assert_eq!(ranking.total, Weight::LowerBound(0));
    Ok(())
}

#[test]
fn ranking_is_stable_and_keeps_unknown_and_other_distinct() -> Result<(), Error> {
    let input = [
        item(8, Weight::Unknown),
        item(4, Weight::Exact(10)),
        item(2, Weight::Exact(50)),
        item(1, Weight::Exact(50)),
        item(3, Weight::LowerBound(5)),
        item(5, Weight::Exact(0)),
    ];
    let result = bar::ranking(
        bounds(),
        &input,
        2,
        Scale::Fixed(50),
        ResultCompleteness::Complete,
    )?;
    assert_eq!(
        result.bars.iter().map(|b| &b.reference).collect::<Vec<_>>(),
        vec![
            &BarRef::Item(1),
            &BarRef::Item(2),
            &BarRef::Other(vec![4, 3, 5])
        ]
    );
    assert_eq!(result.bars[2].value, Weight::LowerBound(15));
    assert_eq!(result.total, Weight::LowerBound(115));
    assert_eq!(result.unknown_items, vec![8]);
    assert!(result.ranking_incomplete);
    assert_eq!(
        result.bars[0].rect,
        Some(Rect {
            left: 10,
            top: 20,
            right: 110,
            bottom: 50
        })
    );
    let mut reversed = input;
    reversed.reverse();
    assert_eq!(
        result,
        bar::ranking(
            bounds(),
            &reversed,
            2,
            Scale::Fixed(50),
            ResultCompleteness::Complete
        )?
    );
    let automatic = bar::ranking(
        bounds(),
        &input,
        2,
        Scale::Automatic,
        ResultCompleteness::Complete,
    )?;
    assert_eq!(automatic.axis_max, None);
    assert_eq!(automatic.bars[0].no_area, Some(NoArea::UncertainScale));
    Ok(())
}

#[test]
fn zero_unknown_overflow_and_input_errors_are_not_fabricated() -> Result<(), Error> {
    let data = [
        item(1, Weight::Exact(u64::MAX)),
        item(2, Weight::Exact(1)),
        item(3, Weight::Exact(0)),
        item(4, Weight::Unknown),
    ];
    let result = bar::ranking(
        bounds(),
        &data,
        0,
        Scale::Automatic,
        ResultCompleteness::Complete,
    )?;
    assert_eq!(result.bars[0].value, Weight::Unknown);
    assert!(result.bars[0].overflowed);
    assert_eq!(result.bars[0].no_area, Some(NoArea::Unknown));
    assert_eq!(result.total, Weight::Unknown);
    assert!(result.total_overflowed);
    assert_eq!(result.unknown_items, vec![4]);
    let zero = bar::ranking(
        bounds(),
        &[item(1, Weight::Exact(0))],
        1,
        Scale::Automatic,
        ResultCompleteness::Complete,
    )?;
    assert_eq!(zero.bars[0].value, Weight::Exact(0));
    assert_eq!(zero.bars[0].no_area, Some(NoArea::ExactZero));
    assert_eq!(
        bar::ranking(
            bounds(),
            &[item(1, Weight::Exact(1)), item(1, Weight::Exact(2))],
            1,
            Scale::Automatic,
            ResultCompleteness::Complete
        ),
        Err(Error::DuplicateItem)
    );
    assert_eq!(
        bar::ranking(
            Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 1
            },
            &[],
            1,
            Scale::Automatic,
            ResultCompleteness::Complete
        ),
        Err(Error::InvalidBounds)
    );
    Ok(())
}

#[test]
fn stacked_order_follows_legend_and_does_not_infer_unknown_offsets() -> Result<(), Error> {
    let legend = [3, 1, 2];
    let input = [
        segment(1, Weight::Exact(20)),
        segment(2, Weight::Exact(30)),
        segment(3, Weight::Exact(50)),
    ];
    let result = bar::stacked(bounds(), 100, &legend, &input)?;
    assert_eq!(result.total, Weight::Exact(100));
    assert_eq!(
        result
            .segments
            .iter()
            .map(|s| s.legend_key)
            .collect::<Vec<_>>(),
        legend
    );
    assert_eq!(
        result.segments[0].rect,
        Some(Rect {
            left: 10,
            top: 20,
            right: 60,
            bottom: 110
        })
    );
    assert_eq!(
        result.segments[1].rect,
        Some(Rect {
            left: 60,
            top: 20,
            right: 80,
            bottom: 110
        })
    );
    let mut reversed = input;
    reversed.reverse();
    assert_eq!(result, bar::stacked(bounds(), 100, &legend, &reversed)?);
    let uncertain = bar::stacked(
        bounds(),
        100,
        &legend,
        &[
            segment(3, Weight::LowerBound(10)),
            segment(1, Weight::Unknown),
            segment(2, Weight::Exact(5)),
        ],
    )?;
    assert_eq!(uncertain.total, Weight::LowerBound(15));
    assert!(uncertain.segments[0].rect.is_some());
    assert_eq!(uncertain.segments[1].no_area, Some(NoArea::Unknown));
    assert_eq!(uncertain.segments[2].no_area, Some(NoArea::UncertainOffset));
    Ok(())
}

#[test]
fn stacked_rejects_missing_or_duplicate_legend_and_invalid_axis() {
    assert_eq!(
        bar::stacked(bounds(), 10, &[1, 1], &[segment(1, Weight::Exact(1))]),
        Err(Error::DuplicateLegend)
    );
    assert_eq!(
        bar::stacked(bounds(), 10, &[1, 2], &[segment(1, Weight::Exact(1))]),
        Err(Error::LegendMismatch)
    );
    assert_eq!(
        bar::stacked(bounds(), 10, &[1], &[segment(1, Weight::Exact(11))]),
        Err(Error::InvalidAxis)
    );
    let overflow = bar::stacked(
        bounds(),
        u64::MAX,
        &[1, 2],
        &[
            segment(1, Weight::Exact(u64::MAX)),
            segment(2, Weight::Exact(1)),
        ],
    );
    assert_eq!(
        overflow.map(|s| (s.total, s.total_overflowed)),
        Ok((Weight::Unknown, true))
    );
}

#[test]
fn partial_source_does_not_claim_a_complete_ranking_or_automatic_scale() -> Result<(), Error> {
    let result = bar::ranking(
        bounds(),
        &[item(1, Weight::Exact(7))],
        1,
        Scale::Automatic,
        ResultCompleteness::Partial,
    )?;
    assert!(result.ranking_incomplete);
    assert_eq!(result.total, Weight::LowerBound(7));
    assert_eq!(result.axis_max, None);
    assert_eq!(result.bars[0].no_area, Some(NoArea::UncertainScale));
    Ok(())
}

#[test]
fn cancellable_ranking_preserves_order_and_interrupts_each_major_stage() -> Result<(), Error> {
    let rows: Vec<_> = (0..64)
        .rev()
        .map(|n| item(n, Weight::Exact(u64::from(n % 7))))
        .collect();
    let result = bar::ranking_cancellable(
        bounds(),
        &rows,
        0,
        Scale::Fixed(1000),
        ResultCompleteness::Complete,
        &mut || false,
    )?;
    let mut expected: Vec<_> = rows.iter().map(|r| (r.item_no, r.value)).collect();
    expected.sort_by(|a, b| {
        let amount = |v| match v {
            Weight::Exact(n) | Weight::LowerBound(n) => n,
            Weight::Unknown => 0,
        };
        amount(b.1).cmp(&amount(a.1)).then_with(|| a.0.cmp(&b.0))
    });
    let BarRef::Other(members) = &result.bars[0].reference else {
        return Err(Error::InvalidBounds);
    };
    assert_eq!(
        *members,
        expected.iter().map(|(n, _)| *n).collect::<Vec<_>>()
    );
    assert_eq!(
        result,
        bar::ranking(
            bounds(),
            &rows,
            0,
            Scale::Fixed(1000),
            ResultCompleteness::Complete
        )?
    );
    for stop in [1, 20, 150, 520, 590, 645] {
        let mut calls = 0;
        let output = bar::ranking_cancellable(
            bounds(),
            &rows,
            0,
            Scale::Fixed(1000),
            ResultCompleteness::Complete,
            &mut || {
                calls += 1;
                calls == stop
            },
        );
        assert_eq!(output, Err(Error::Cancelled), "checkpoint {stop}");
    }
    assert_eq!(
        result
            .geometry_v1_cancellable(&mut || false)
            .map_err(|_| Error::InvalidBounds)?,
        result.geometry_v1().map_err(|_| Error::InvalidBounds)?
    );
    assert_eq!(
        result.geometry_v1_cancellable(&mut || true).err(),
        Some(bar::GeometryError::Cancelled)
    );
    Ok(())
}

#[test]
fn cancellable_stack_preserves_legend_order_and_interrupts_each_stage() -> Result<(), Error> {
    let legend: Vec<_> = (0..64).collect();
    let input: Vec<_> = legend
        .iter()
        .rev()
        .map(|n| segment(*n, Weight::Exact(1)))
        .collect();
    let result = bar::stacked_cancellable(bounds(), 64, &legend, &input, &mut || false)?;
    assert_eq!(result, bar::stacked(bounds(), 64, &legend, &input)?);
    for stop in [1, 20, 75, 140, 194] {
        let mut calls = 0;
        let output = bar::stacked_cancellable(bounds(), 64, &legend, &input, &mut || {
            calls += 1;
            calls == stop
        });
        assert_eq!(output, Err(Error::Cancelled), "checkpoint {stop}");
    }
    assert_eq!(
        result
            .geometry_v1_cancellable(&mut || false)
            .map_err(|_| Error::InvalidBounds)?,
        result.geometry_v1().map_err(|_| Error::InvalidBounds)?
    );
    let mut calls = 0;
    assert_eq!(
        result
            .geometry_v1_cancellable(&mut || {
                calls += 1;
                calls == 30
            })
            .err(),
        Some(bar::GeometryError::Cancelled)
    );
    Ok(())
}

#[test]
fn large_unknown_sort_is_cancellable_and_keeps_canonical_order() -> Result<(), Error> {
    let input: Vec<_> = (0..8192).rev().map(|n| item(n, Weight::Unknown)).collect();
    let mut checks = 0;
    assert_eq!(
        bar::ranking_cancellable(
            bounds(),
            &input,
            1,
            Scale::Automatic,
            ResultCompleteness::Complete,
            &mut || {
                checks += 1;
                checks == 10_000
            }
        ),
        Err(Error::Cancelled)
    );
    let result = bar::ranking_cancellable(
        bounds(),
        &input,
        1,
        Scale::Automatic,
        ResultCompleteness::Complete,
        &mut || false,
    )?;
    assert_eq!(result.unknown_items, (0..8192).collect::<Vec<_>>());
    Ok(())
}
