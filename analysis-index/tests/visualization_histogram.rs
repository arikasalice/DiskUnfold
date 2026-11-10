use du_analysis_index::visualization::{
    bar::{NoArea, Scale},
    histogram::{self, Basis, Bin, Error, Item},
    squarify::{Rect, Weight},
};
use du_core_contracts::result::ResultCompleteness;

fn bounds() -> Rect {
    Rect {
        left: 0,
        top: 0,
        right: 100,
        bottom: 90,
    }
}
fn bins() -> [Bin; 3] {
    [
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
    ]
}
fn item(item_no: u32, value: Weight) -> Item {
    Item { item_no, value }
}

#[test]
fn half_open_bins_preserve_count_and_bytes_and_ignore_input_order() -> Result<(), Error> {
    let mut input = [
        item(0, Weight::Exact(0)),
        item(1, Weight::Exact(9)),
        item(2, Weight::Exact(10)),
        item(3, Weight::Exact(19)),
        item(4, Weight::Exact(20)),
        item(5, Weight::Exact(25)),
    ];
    let result = histogram::layout(
        bounds(),
        &bins(),
        &input,
        Basis::Bytes,
        Scale::Automatic,
        ResultCompleteness::Complete,
    )?;
    assert_eq!(
        result.bins.iter().map(|b| b.item_count).collect::<Vec<_>>(),
        vec![Weight::Exact(2); 3]
    );
    assert_eq!(
        result.bins.iter().map(|b| b.bytes).collect::<Vec<_>>(),
        vec![Weight::Exact(9), Weight::Exact(29), Weight::Exact(45)]
    );
    assert_eq!(result.total_bytes, Weight::Exact(83));
    assert_eq!(
        result.bins[2].rect,
        Some(Rect {
            left: 0,
            top: 60,
            right: 100,
            bottom: 90
        })
    );
    input.reverse();
    assert_eq!(
        result,
        histogram::layout(
            bounds(),
            &bins(),
            &input,
            Basis::Bytes,
            Scale::Automatic,
            ResultCompleteness::Complete
        )?
    );
    let count = histogram::layout(
        bounds(),
        &bins(),
        &input,
        Basis::ItemCount,
        Scale::Automatic,
        ResultCompleteness::Complete,
    )?;
    assert!(
        count
            .bins
            .iter()
            .all(|b| b.rect.is_some_and(|r| r.right == 100))
    );
    Ok(())
}

#[test]
fn unknown_and_ambiguous_lower_bounds_are_not_placed_as_zero() -> Result<(), Error> {
    let input = [
        item(1, Weight::Exact(0)),
        item(2, Weight::LowerBound(15)),
        item(3, Weight::LowerBound(20)),
        item(4, Weight::Unknown),
    ];
    let result = histogram::layout(
        bounds(),
        &bins(),
        &input,
        Basis::Bytes,
        Scale::Automatic,
        ResultCompleteness::Complete,
    )?;
    assert_eq!(result.uncertain_items, vec![2, 4]);
    assert_eq!(result.bins[0].item_count, Weight::LowerBound(1));
    assert_eq!(result.bins[0].bytes, Weight::LowerBound(0));
    assert_eq!(result.bins[1].item_count, Weight::Unknown);
    assert!(result.bins[1].rect.is_none());
    assert_eq!(result.bins[2].item_count, Weight::LowerBound(1));
    assert_eq!(result.bins[2].bytes, Weight::LowerBound(20));
    assert_eq!(result.total_bytes, Weight::LowerBound(35));
    assert_eq!(result.axis_max, None);
    assert_eq!(result.bins[2].no_area, Some(NoArea::UncertainScale));
    let fixed = histogram::layout(
        bounds(),
        &bins(),
        &input,
        Basis::Bytes,
        Scale::Fixed(20),
        ResultCompleteness::Complete,
    )?;
    assert!(fixed.bins[2].rect.is_some());
    Ok(())
}

#[test]
fn overflow_and_unclassified_values_remain_explicit() -> Result<(), Error> {
    let one = [Bin {
        lower: 0,
        upper: None,
    }];
    let input = [item(1, Weight::Exact(u64::MAX)), item(2, Weight::Exact(1))];
    let bytes = histogram::layout(
        bounds(),
        &one,
        &input,
        Basis::Bytes,
        Scale::Automatic,
        ResultCompleteness::Complete,
    )?;
    assert_eq!(bytes.bins[0].bytes, Weight::Unknown);
    assert!(bytes.bins[0].bytes_overflowed);
    assert!(bytes.bins[0].rect.is_none());
    assert_eq!(bytes.total_bytes, Weight::Unknown);
    assert!(bytes.total_bytes_overflowed);
    let counts = histogram::layout(
        bounds(),
        &one,
        &input,
        Basis::ItemCount,
        Scale::Automatic,
        ResultCompleteness::Complete,
    )?;
    assert_eq!(counts.bins[0].item_count, Weight::Exact(2));
    assert!(counts.bins[0].rect.is_some());
    let partial = [Bin {
        lower: 10,
        upper: Some(20),
    }];
    let result = histogram::layout(
        bounds(),
        &partial,
        &[item(3, Weight::Exact(9)), item(2, Weight::Exact(20))],
        Basis::Bytes,
        Scale::Automatic,
        ResultCompleteness::Complete,
    )?;
    assert_eq!(result.unclassified_items, vec![2, 3]);
    assert_eq!(result.bins[0].item_count, Weight::Exact(0));
    Ok(())
}

#[test]
fn invalid_boundaries_and_duplicate_items_are_rejected() {
    assert_eq!(
        histogram::layout(
            bounds(),
            &[],
            &[],
            Basis::Bytes,
            Scale::Automatic,
            ResultCompleteness::Complete
        ),
        Err(Error::InvalidBins)
    );
    assert_eq!(
        histogram::layout(
            bounds(),
            &[Bin {
                lower: 2,
                upper: Some(2)
            }],
            &[],
            Basis::Bytes,
            Scale::Automatic,
            ResultCompleteness::Complete
        ),
        Err(Error::InvalidBins)
    );
    assert_eq!(
        histogram::layout(
            bounds(),
            &[
                Bin {
                    lower: 0,
                    upper: Some(11)
                },
                Bin {
                    lower: 10,
                    upper: None
                }
            ],
            &[],
            Basis::Bytes,
            Scale::Automatic,
            ResultCompleteness::Complete
        ),
        Err(Error::InvalidBins)
    );
    assert_eq!(
        histogram::layout(
            bounds(),
            &bins(),
            &[item(1, Weight::Exact(1)), item(1, Weight::Exact(2))],
            Basis::Bytes,
            Scale::Automatic,
            ResultCompleteness::Complete
        ),
        Err(Error::DuplicateItem)
    );
}

#[test]
fn partial_source_preserves_known_bounds_without_fabricating_empty_bins() -> Result<(), Error> {
    let result = histogram::layout(
        bounds(),
        &bins(),
        &[item(1, Weight::Exact(5))],
        Basis::ItemCount,
        Scale::Automatic,
        ResultCompleteness::Partial,
    )?;
    assert_eq!(result.observed_items, 1);
    assert_eq!(result.bins[0].item_count, Weight::LowerBound(1));
    assert_eq!(result.bins[1].item_count, Weight::Unknown);
    assert_eq!(result.total_bytes, Weight::LowerBound(5));
    assert_eq!(result.axis_max, None);
    assert_eq!(result.bins[0].no_area, Some(NoArea::UncertainScale));
    Ok(())
}
