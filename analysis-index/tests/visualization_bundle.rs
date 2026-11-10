use du_analysis_index::{
    IngestSink, Observation,
    visualization::{
        bundle::{self, Error},
        geometry::Rect,
        registry,
        treemap::ChildKey,
        typed_layout::{self, Input, Strategy, Treemap},
    },
};
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
    visualization::{LayoutDescriptor, LayoutStrategyKey},
};

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

#[test]
fn full_bundle_rejects_missing_and_duplicate_registration() -> Result<(), String> {
    let empty = registry::Builder::<Box<dyn Strategy>>::default();
    assert!(matches!(
        bundle::finish_default(empty),
        Err(Error::MissingRequired("viz.treemap"))
    ));
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    bundle::register_implemented(&mut builder).map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        bundle::register_implemented(&mut builder),
        Err(Error::Registration(
            registry::RegistrationError::DuplicateStrategy
        ))
    ));
    assert!(matches!(
        bundle::finish_default(builder),
        Err(Error::MissingRequired("viz.bar"))
    ));
    Ok(())
}

#[test]
fn wrong_treemap_format_cannot_satisfy_default_registration() -> Result<(), String> {
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    let descriptor = LayoutDescriptor {
        format: "fixture.wrong".into(),
        strategy: LayoutStrategyKey {
            id: "viz.treemap".into(),
            revision: 1,
        },
    };
    builder
        .register(descriptor, Box::new(Treemap))
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        bundle::finish_default(builder),
        Err(Error::MissingRequired("viz.treemap"))
    ));
    Ok(())
}

#[test]
fn partial_dispatcher_executes_the_actual_treemap() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut builder = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    builder
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    builder
        .add_file(Observation {
            path: vec![b"a".to_vec()],
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: SizeAmount::Exact {
                    bytes: 10_u64.into(),
                },
            }),
        })
        .map_err(|e| format!("{e:?}"))?;
    let version = builder
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let item = |n| ItemRef {
        result_version: version.version_id().clone(),
        item_no: n,
    };
    let keys = vec![ChildKey {
        item: item(1),
        path_compare_key: b"a".to_vec(),
    }];
    let input = Input::admit(
        &version,
        &item(0),
        SizeBasis::Logical,
        Rect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        },
        &keys,
    )
    .map_err(|e| format!("{e:?}"))?;
    let dispatcher = bundle::implemented_dispatcher().map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        dispatcher.catalog().cloned().collect::<Vec<_>>(),
        [typed_layout::treemap_descriptor()]
    );
    let selected = dispatcher
        .execute("treemap", None, &input, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    let expected = Treemap
        .layout(&input, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(selected.descriptor, typed_layout::treemap_descriptor());
    assert_eq!(selected.output, expected);
    assert_eq!(selected.output.shapes.len(), 1);
    assert_eq!(selected.output.shapes[0].geometry.primitive(), "rect");
    assert!(matches!(
        dispatcher.execute("bar", None, &input, &mut || false),
        Err(typed_layout::DispatchError::Selection(
            registry::SelectionError::UnknownFormat
        ))
    ));
    Ok(())
}
