use du_analysis_index::visualization::{
    bar::{self, BarRef, Error, Item, NoArea, Scale, Segment},
    squarify::{Rect, Weight},
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
    assert_eq!(zero.bars[0].no_area, Some(NoArea::Zero));
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
