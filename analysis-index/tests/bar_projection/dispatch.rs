use super::*;

#[test]
fn ranked_bar_dispatch_preserves_evidence_and_rejects_unimplemented_modes() -> Result<(), String> {
    use du_analysis_index::visualization::{
        bundle, registry,
        typed_layout::{self, BarDispatchError, BarError, BarInput, Dispatcher, Strategy},
    };
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    bundle::register_implemented(&mut builder).map_err(|e| format!("{e:?}"))?;
    bundle::register_ranked_bar(&mut builder).map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        bundle::register_ranked_bar(&mut builder),
        Err(bundle::Error::Registration(
            registry::RegistrationError::DuplicateStrategy
        ))
    ));
    assert!(matches!(
        bundle::finish_default(builder),
        Err(bundle::Error::MissingRequired("viz.bar"))
    ));

    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    bundle::register_ranked_bar(&mut builder).map_err(|e| format!("{e:?}"))?;
    let dispatcher = Dispatcher::new(
        builder
            .build(&[typed_layout::bar_descriptor()])
            .map_err(|e| format!("{e:?}"))?,
    );
    let version = fixture(ResultCompleteness::Complete)?;
    let scope = reference(&version, 0);
    let input = || ChildrenRequest {
        version: &version,
        scope: &scope,
        basis: SizeBasis::Logical,
        bounds: Rect {
            left: 0,
            top: 0,
            right: 1_000_000,
            bottom: 1_000_000,
        },
        scale: Scale::Fixed(50),
        top: 2,
        max_bars: 3,
    };
    let selected = dispatcher
        .execute_bar("bar", None, BarInput::Ranked(input()), &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    check_prepared_dispatch(&dispatcher, input(), &selected)?;
    let mut full_builder = registry::Builder::<Box<dyn Strategy>>::default();
    bundle::register_bar_candidate(&mut full_builder).map_err(|e| format!("{e:?}"))?;
    let full_dispatcher = Dispatcher::new(
        full_builder
            .build(&[typed_layout::bar_descriptor()])
            .map_err(|e| format!("{e:?}"))?,
    );
    check_prepared_dispatch(&full_dispatcher, input(), &selected)?;
    assert_eq!(selected.descriptor, typed_layout::bar_descriptor());
    assert_eq!(
        selected.output,
        bar_projection::project_children(input(), &mut || false).map_err(|e| format!("{e:?}"))?
    );
    assert_eq!(selected.output.denominator, aggregate(&version)?);
    assert!(matches!(
        selected.output.bars[2].identity,
        BarIdentity::Other { .. }
    ));
    assert!(!selected.output.unknown_items.is_empty());
    for mode in [BarInput::TypeComparison, BarInput::Stacked] {
        assert!(matches!(
            dispatcher.execute_bar("bar", None, mode, &mut || false),
            Err(BarDispatchError::Bar(BarError::UnsupportedMode))
        ));
    }
    assert!(matches!(
        dispatcher.execute_bar("bar", None, BarInput::Ranked(input()), &mut || true),
        Err(BarDispatchError::Bar(BarError::Projection(
            Error::Cancelled
        )))
    ));
    let mut wrong = typed_layout::bar_descriptor().strategy;
    wrong.revision += 1;
    assert!(matches!(
        dispatcher.execute_bar("bar", Some(&wrong), BarInput::Ranked(input()), &mut || {
            false
        }),
        Err(BarDispatchError::Selection(
            registry::SelectionError::UnknownStrategy
        ))
    ));
    assert!(matches!(
        dispatcher.execute_bar("other", None, BarInput::Ranked(input()), &mut || false),
        Err(BarDispatchError::Selection(
            registry::SelectionError::UnknownFormat
        ))
    ));
    Ok(())
}

fn check_prepared_dispatch(
    dispatcher: &du_analysis_index::visualization::typed_layout::Dispatcher,
    request: ChildrenRequest<'_>,
    selected: &du_analysis_index::visualization::typed_layout::SelectedBar,
) -> Result<(), String> {
    use du_analysis_index::visualization::typed_layout::{self, BarInput};
    let prepared =
        bar_projection::prepare_children(request, &mut || false).map_err(|e| format!("{e:?}"))?;
    let staged = dispatcher
        .execute_bar(
            "bar",
            Some(&typed_layout::bar_descriptor().strategy),
            BarInput::PreparedRanked(Box::new(prepared)),
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(staged.descriptor, selected.descriptor);
    assert_eq!(staged.output, selected.output);
    Ok(())
}
