use du_analysis_index::visualization::{
    geometry::{NoArea, Rect, Scale, Weight},
    waterfall::{self, Change, Direction, Error},
};

fn bounds() -> Rect {
    Rect {
        left: 0,
        top: 0,
        right: 120,
        bottom: 100,
    }
}

fn change(item_no: u32, direction: Direction, value: Weight) -> Change {
    Change {
        item_no,
        direction,
        value,
    }
}

#[test]
fn lays_out_start_ordered_changes_and_end_on_a_shared_axis() -> Result<(), Error> {
    let changes = [
        change(10, Direction::Increase, Weight::Exact(40)),
        change(11, Direction::Decrease, Weight::Exact(30)),
    ];
    let result = waterfall::layout(
        bounds(),
        Weight::Exact(100),
        &changes,
        Weight::Exact(110),
        Scale::Automatic,
    )?;

    assert_eq!(result.axis_max, Some(140));
    assert_eq!(
        result.start.rect,
        Some(Rect {
            left: 0,
            top: 29,
            right: 30,
            bottom: 100
        })
    );
    assert_eq!(result.changes[0].item_no, Some(10));
    assert_eq!(result.start.direction, None);
    assert_eq!(result.changes[0].direction, Some(Direction::Increase));
    assert_eq!(
        result.changes[0].rect,
        Some(Rect {
            left: 30,
            top: 0,
            right: 60,
            bottom: 29
        })
    );
    assert_eq!(result.changes[1].item_no, Some(11));
    assert_eq!(result.changes[1].direction, Some(Direction::Decrease));
    assert_eq!(
        result.changes[1].rect,
        Some(Rect {
            left: 60,
            top: 0,
            right: 90,
            bottom: 22
        })
    );
    assert_eq!(
        result.end.rect,
        Some(Rect {
            left: 90,
            top: 22,
            right: 120,
            bottom: 100
        })
    );
    assert!(result.uncertain_changes.is_empty());
    Ok(())
}

#[test]
fn unknown_change_is_reported_and_makes_later_offsets_uncertain() -> Result<(), Error> {
    let changes = [
        change(1, Direction::Increase, Weight::Exact(5)),
        change(2, Direction::Decrease, Weight::Unknown),
        change(3, Direction::Increase, Weight::Exact(2)),
    ];
    let result = waterfall::layout(
        bounds(),
        Weight::Exact(10),
        &changes,
        Weight::Exact(9),
        Scale::Fixed(20),
    )?;

    assert_eq!(result.axis_max, Some(20));
    assert_eq!(
        result.changes[0].rect,
        Some(Rect {
            left: 24,
            top: 25,
            right: 48,
            bottom: 50
        })
    );
    assert_eq!(result.changes[1].no_area, Some(NoArea::Unknown));
    assert_eq!(result.changes[1].direction, Some(Direction::Decrease));
    assert_eq!(result.changes[2].no_area, Some(NoArea::UncertainOffset));
    assert_eq!(result.uncertain_changes, vec![changes[1]]);
    assert!(result.end.rect.is_some());
    Ok(())
}

#[test]
fn lower_bound_totals_remain_without_geometry_and_do_not_become_zero() -> Result<(), Error> {
    let changes = [change(7, Direction::Increase, Weight::Exact(1))];
    let result = waterfall::layout(
        bounds(),
        Weight::LowerBound(8),
        &changes,
        Weight::LowerBound(9),
        Scale::Automatic,
    )?;

    assert_eq!(result.axis_max, None);
    assert_eq!(result.start.no_area, Some(NoArea::Unknown));
    assert_eq!(result.changes[0].no_area, Some(NoArea::UncertainOffset));
    assert_eq!(result.end.no_area, Some(NoArea::Unknown));
    Ok(())
}

#[test]
fn rejects_duplicate_items_and_fixed_axes_below_known_values() {
    let duplicate = [
        change(3, Direction::Increase, Weight::Exact(1)),
        change(3, Direction::Decrease, Weight::Exact(1)),
    ];
    assert_eq!(
        waterfall::layout(
            bounds(),
            Weight::Exact(2),
            &duplicate,
            Weight::Exact(2),
            Scale::Automatic
        ),
        Err(Error::DuplicateItem)
    );
    let change = [change(1, Direction::Increase, Weight::Exact(5))];
    assert_eq!(
        waterfall::layout(
            bounds(),
            Weight::Exact(10),
            &change,
            Weight::Exact(15),
            Scale::Fixed(14)
        ),
        Err(Error::InvalidAxis)
    );
}
