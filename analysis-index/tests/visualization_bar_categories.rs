use du_analysis_index::visualization::{
    bar::{self, BarRef},
    bar_categories::{Error, Input, Layout, Row},
    bundle,
    geometry::{NoArea, Rect, Scale, Weight},
    registry,
    typed_layout::{self, BarDispatchError, BarError, Dispatcher, Strategy},
};
use du_core_contracts::{result::ResultCompleteness, visualization::quantity::VizQuantity};

const BOUNDS: Rect = Rect {
    left: 0,
    top: 0,
    right: 100,
    bottom: 100,
};

fn dispatcher() -> Result<Dispatcher, String> {
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    bundle::register_bar_candidate(&mut builder).map_err(|error| format!("{error:?}"))?;
    Ok(Dispatcher::new(
        builder
            .build(&[typed_layout::bar_descriptor()])
            .map_err(|error| format!("{error:?}"))?,
    ))
}

fn bytes(legend_key: u32, value: i128) -> Result<Row, String> {
    Ok(Row {
        legend_key,
        quantity: VizQuantity::exact("bytes", value).ok_or("invalid quantity")?,
    })
}

#[test]
fn complete_bar_strategy_satisfies_only_its_default_requirement() -> Result<(), String> {
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    bundle::register_implemented(&mut builder).map_err(|error| format!("{error:?}"))?;
    bundle::register_bar_candidate(&mut builder).map_err(|error| format!("{error:?}"))?;
    assert!(matches!(
        bundle::finish_default(builder),
        Err(bundle::Error::MissingRequired("viz.trend"))
    ));
    Ok(())
}

#[test]
fn registered_type_comparison_retains_order_and_unknown_quantity() -> Result<(), String> {
    let dispatcher = dispatcher()?;
    let rows = [
        bytes(2, 10)?,
        bytes(3, 25)?,
        bytes(1, 10)?,
        Row {
            legend_key: 4,
            quantity: VizQuantity::unknown("bytes", "result.unobserved".into())
                .ok_or("invalid quantity")?,
        },
    ];
    let selected = dispatcher
        .execute_category_bar(
            None,
            Input::Comparison {
                bounds: BOUNDS,
                rows: &rows,
                scale: Scale::Fixed(50),
                completeness: ResultCompleteness::Complete,
            },
            &mut || false,
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(selected.descriptor, typed_layout::bar_descriptor());
    let Layout::Comparison(layout) = selected.output else {
        return Err("wrong layout".into());
    };
    assert_eq!(
        layout
            .bars
            .iter()
            .map(|bar| &bar.reference)
            .collect::<Vec<_>>(),
        [&BarRef::Item(3), &BarRef::Item(1), &BarRef::Item(2),]
    );
    assert_eq!(layout.unknown_items, [4]);
    assert_eq!(layout.total, Weight::LowerBound(45));
    assert!(layout.ranking_incomplete);
    Ok(())
}

#[test]
fn registered_stack_preserves_legend_order_and_uncertain_offsets() -> Result<(), String> {
    let dispatcher = dispatcher()?;
    let rows = [
        bytes(2, 10)?,
        bytes(3, 25)?,
        Row {
            legend_key: 1,
            quantity: VizQuantity::lower_bound("bytes", 5).ok_or("invalid quantity")?,
        },
    ];
    let input = |legend| Input::Stacked {
        bounds: BOUNDS,
        rows: &rows,
        axis_max: 50,
        legend,
    };
    let selected = dispatcher
        .execute_category_bar(
            Some(&typed_layout::bar_descriptor().strategy),
            input(&[3, 1, 2]),
            &mut || false,
        )
        .map_err(|error| format!("{error:?}"))?;
    let Layout::Stacked(layout) = selected.output else {
        return Err("wrong layout".into());
    };
    assert_eq!(
        layout
            .segments
            .iter()
            .map(|segment| segment.legend_key)
            .collect::<Vec<_>>(),
        [3, 1, 2]
    );
    assert_eq!(layout.total, Weight::LowerBound(40));
    assert_eq!(layout.segments[2].no_area, Some(NoArea::UncertainOffset));
    assert!(matches!(
        dispatcher.execute_category_bar(None, input(&[3, 1]), &mut || false),
        Err(BarDispatchError::Bar(BarError::Category(Error::Layout(
            bar::Error::LegendMismatch
        ))))
    ));
    assert!(matches!(
        dispatcher.execute_category_bar(None, input(&[3, 1, 2]), &mut || true),
        Err(BarDispatchError::Bar(BarError::Category(Error::Layout(
            bar::Error::Cancelled
        ))))
    ));
    Ok(())
}

#[test]
fn classified_sizes_reject_negative_and_non_byte_quantities() -> Result<(), String> {
    let dispatcher = dispatcher()?;
    for row in [
        bytes(1, -1)?,
        Row {
            legend_key: 1,
            quantity: VizQuantity::exact("items", 10).ok_or("invalid quantity")?,
        },
    ] {
        let rows = [row];
        assert!(matches!(
            dispatcher.execute_category_bar(
                None,
                Input::Comparison {
                    bounds: BOUNDS,
                    rows: &rows,
                    scale: Scale::Automatic,
                    completeness: ResultCompleteness::Complete,
                },
                &mut || false
            ),
            Err(BarDispatchError::Bar(BarError::Category(
                Error::InvalidQuantity
            )))
        ));
    }
    Ok(())
}
