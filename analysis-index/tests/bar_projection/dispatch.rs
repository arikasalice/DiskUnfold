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
